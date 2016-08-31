use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::sync::mpsc::Sender;
use std::collections::HashMap;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use events::push_notification::PushNotification;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use notifier::Notifier;
use producer::FcmData;
use certificate_registry::{CertificateRegistry, CertificateError};
use time::precise_time_s;

pub struct Consumer<'a> {
    channel: Mutex<Channel>,
    session: Session,
    notifier: Notifier<'a>,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    tx: Sender<FcmData>,
    registry: Arc<CertificateRegistry>,
    cache_ttl: f64,
}

struct ApiKey {
    pub key: Option<String>,
    pub timestamp: f64,
}

impl<'a> Drop for Consumer<'a> {
    fn drop(&mut self) {
        let mut channel = self.channel.lock().unwrap();

        let _ = channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl<'a> Consumer<'a> {
    pub fn new(control: Arc<AtomicBool>, config: Arc<Config>, notifier: Notifier<'a>,
               tx: Sender<FcmData>, registry: Arc<CertificateRegistry>) -> Consumer {
        let mut session = Session::new(Options {
            vhost: &config.rabbitmq.vhost,
            host: &config.rabbitmq.host,
            port: config.rabbitmq.port,
            login: &config.rabbitmq.login,
            password: &config.rabbitmq.password, .. Default::default()
        }).unwrap();

        let mut channel = session.open_channel(1).unwrap();

        channel.queue_declare(
            &*config.rabbitmq.queue,
            false, // passive
            true,  // durable
            false, // exclusive
            false, // auto_delete
            false, // nowait
            Table::new()).unwrap();

        channel.exchange_declare(
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).unwrap();

        channel.queue_bind(
            &*config.rabbitmq.queue,
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.routing_key,
            false, // nowait
            Table::new()).unwrap();

        Consumer {
            channel: Mutex::new(channel),
            session: session,
            notifier: notifier,
            control: control,
            config: config,
            tx: tx,
            registry: registry,
            cache_ttl: 10.0,
        }
    }

    pub fn consume(&self) -> Result<(), Error> {
        let mut certificates: HashMap<String, ApiKey> = HashMap::new();
        let mut channel = self.channel.lock().unwrap();

        while self.control.load(Ordering::Relaxed) {
            for result in channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    self.update_certificates(&mut certificates, event.get_application_id());

                    let response = match certificates.get(event.get_application_id()) {
                        Some(&ApiKey { key: Some(ref key), timestamp: _ }) => Some(self.notifier.send(&event, key)),
                        _ => None,
                    };

                    self.tx.send((event, response)).unwrap();
                } else {
                    error!("Broken protobuf data");
                }

                result.ack();

                if !self.control.load(Ordering::Relaxed) { break; }
            }

            thread::park_timeout(Duration::from_millis(100));
        }

        Ok(())
    }

    fn update_certificates(&self, certificates: &mut HashMap<String, ApiKey>, application_id: &str) {
        let fetch_key = |api_key: &str| {
            ApiKey {
                key: Some(String::from(api_key)),
                timestamp: precise_time_s(),
            }
        };

        let add_key = move |result: Result<ApiKey, CertificateError>, certificates: &mut HashMap<String, ApiKey>| {
            match result {
                Ok(api_key) => {
                    certificates.insert(String::from(application_id), api_key);
                },
                Err(err) => {
                    error!("Error when fetching certificate for {}: {:?}", application_id, err);

                    certificates.insert(String::from(application_id), ApiKey {
                        key: None,
                        timestamp: precise_time_s(),
                    });
                },
            }
        };

        if certificates.get(application_id).is_some() && self.is_expired(certificates.get(application_id).unwrap()) {
            certificates.remove(application_id);
            add_key(self.registry.fetch(application_id, fetch_key), certificates);
        } else if certificates.get(application_id).is_none() {
            add_key(self.registry.fetch(application_id, fetch_key), certificates);
        }
    }

    fn is_expired(&self, key: &ApiKey) -> bool {
        precise_time_s() - key.timestamp >= self.cache_ttl
    }
}

