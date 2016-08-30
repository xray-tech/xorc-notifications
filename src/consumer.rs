use std::sync::Arc;
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
use certificate_registry::CertificateRegistry;
use time::precise_time_s;

pub struct Consumer<'a> {
    channel: Channel,
    session: Session,
    notifier: Notifier<'a>,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    tx: Sender<FcmData>,
    registry: Arc<CertificateRegistry>,
}

struct ApiKey {
    pub key: Option<String>,
    pub timestamp: f64,
}

impl<'a> Drop for Consumer<'a> {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
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
            channel: channel,
            session: session,
            notifier: notifier,
            control: control,
            config: config,
            tx: tx,
            registry: registry,
        }
    }

    pub fn consume(&mut self) -> Result<(), Error> {
        let mut certificates: HashMap<String, ApiKey> = HashMap::new();
        let cache_ttl = 10.0;

        while self.control.load(Ordering::Relaxed) {
            for result in self.channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    if let Some(api_key) = {
                        let application_id = event.get_application_id();

                        let expired_keys: Vec<_> = certificates
                            .iter()
                            .filter(|&(_, ref v)| precise_time_s() - v.timestamp >= cache_ttl)
                            .map(|(k, _)| k.clone())
                            .collect();

                        for key in expired_keys { certificates.remove(&key); }

                        if !certificates.contains_key(application_id) {
                            let add_key = |api_key: &str| {
                                ApiKey {
                                    key: Some(String::from(api_key)),
                                    timestamp: precise_time_s(),
                                }
                            };

                            match self.registry.fetch(application_id, add_key) {
                                Ok(api_key) => {
                                    certificates.insert(String::from(application_id), api_key);
                                },
                                Err(err) => {
                                    error!("Error when fetching certificate for {}: {:?}", event.get_application_id(), err);

                                    certificates.insert(String::from(application_id), ApiKey {
                                        key: None,
                                        timestamp: precise_time_s(),
                                    });
                                },
                            }
                        }

                        certificates.get(application_id)
                    } {
                        match api_key.key {
                            Some(ref key) => {
                                let response = self.notifier.send(&event, key);
                                self.tx.send((event, Some(response))).unwrap();
                            },
                            None => {
                                self.tx.send((event, None)).unwrap();
                            }
                        }
                    } else {
                        self.tx.send((event, None)).unwrap();
                    }
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
}

