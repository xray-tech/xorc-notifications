use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::collections::HashMap;
use std::thread::park_timeout;
use time::precise_time_s;
use std::time::Duration;
use futures::{Sink, Future};
use futures::sync::mpsc::Sender;
use amqp::{Session, Channel, Table, Basic, Options, Consumer as AmqpConsumer};
use amqp::protocol::basic;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use certificate_registry::{CertificateRegistry, CertificateError};
use events::push_notification::PushNotification;
use notifier::NotifierMessage;
use metrics::CALLBACKS_INFLIGHT;

pub struct Consumer {
    channel: Mutex<Channel>,
    session: Session,
    config: Arc<Config>,
    registry: Arc<CertificateRegistry>,
}

struct ApiKey {
    pub key: Result<Option<String>, CertificateError>,
    pub timestamp: f64,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let mut channel = self.channel.lock().unwrap();

        let _ = channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl Consumer {
    pub fn new(control: Arc<AtomicBool>,
               config: Arc<Config>,
               registry: Arc<CertificateRegistry>) -> Consumer {
        let mut session = Session::new(Options {
            vhost: config.rabbitmq.vhost.clone(),
            host: config.rabbitmq.host.clone(),
            port: config.rabbitmq.port,
            login: config.rabbitmq.login.clone(),
            password: config.rabbitmq.password.clone(), .. Default::default()
        }, control.clone()).unwrap();

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
            config: config,
            registry: registry,
        }
    }

    pub fn consume(&self, notifier_tx: Sender<NotifierMessage>) -> Result<(), Error> {
        let mut channel = self.channel.lock().unwrap();
        let consumer = WebPushConsumer::new(notifier_tx, self.registry.clone());

        channel.basic_prefetch(100).ok().expect("failed to prefetch");

        let consumer_name = channel.basic_consume(consumer,
                                                  &*self.config.rabbitmq.queue,
                                                  "webpush_consumer",
                                                  true,  // no local
                                                  false, // no ack
                                                  false, // exclusive
                                                  false, // nowait
                                                  Table::new());

        info!("Starting consumer {:?}", consumer_name);
        channel.start_consuming();

        Ok(())
    }
}

struct WebPushConsumer {
    registry: Arc<CertificateRegistry>,
    notifier_tx: Sender<NotifierMessage>,
    certificates: HashMap<String, ApiKey>,
    cache_ttl: f64,
}

impl WebPushConsumer {
    pub fn new(notifier_tx: Sender<NotifierMessage>, registry: Arc<CertificateRegistry>) -> WebPushConsumer {
        WebPushConsumer {
            notifier_tx: notifier_tx,
            certificates: HashMap::new(),
            registry: registry,
            cache_ttl: 120.0,
        }
    }

    fn update_certificates(&mut self, application_id: &str) {
        let fetch_key = |api_key: Option<String>| {
            ApiKey {
                key: Ok(api_key),
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
                        key: Err(err),
                        timestamp: precise_time_s(),
                    });
                },
            }
        };

        if self.certificates.get(application_id).is_some() && self.is_expired(self.certificates.get(application_id).unwrap()) {
            self.certificates.remove(application_id);
            add_key(self.registry.fetch(application_id, fetch_key), &mut self.certificates);
        } else if self.certificates.get(application_id).is_none() {
            add_key(self.registry.fetch(application_id, fetch_key), &mut self.certificates);
        }
    }

    fn is_expired(&self, key: &ApiKey) -> bool {
        precise_time_s() - key.timestamp >= self.cache_ttl
    }
}

impl AmqpConsumer for WebPushConsumer {
    fn handle_delivery(&mut self, channel: &mut Channel, deliver: basic::Deliver,
                       _headers: basic::BasicProperties, body: Vec<u8>) {
        if CALLBACKS_INFLIGHT.get() < 10000.0 {
            channel.basic_ack(deliver.delivery_tag, false).unwrap();

            if let Ok(event) = parse_from_bytes::<PushNotification>(&body) {
                CALLBACKS_INFLIGHT.inc();

                self.update_certificates(event.get_application_id());
                let nx = self.notifier_tx.clone();

                match self.certificates.get(event.get_application_id()) {
                    Some(&ApiKey {key: Ok(Some(ref gcm_api_key)), timestamp: _ }) =>
                        nx.send((Ok(Some(gcm_api_key.to_string())), event)).wait().unwrap(),
                    Some(&ApiKey {key: Ok(None), timestamp: _ }) =>
                        nx.send((Ok(None), event)).wait().unwrap(),
                    Some(&ApiKey {key: Err(_), timestamp: _ }) =>
                        nx.send((Err(()), event)).wait().unwrap(),
                    None => {
                        nx.send((Err(()), event)).wait().unwrap()
                    }
                };
            } else {
                error!("Broken protobuf data");
            }
        } else {
            error!("ERROR: Too many callbacks in-flight, requeuing");
            park_timeout(Duration::from_millis(1000));
            channel.basic_nack(deliver.delivery_tag, false, true).unwrap();
        }
    }
}
