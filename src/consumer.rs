use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::io::Cursor;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use events::push_notification::PushNotification;
use notifier::Apns2Notifier;
use apns2::AsyncResponse;
use protobuf::parse_from_bytes;
use config::Config;
use metrics::Metrics;
use hyper::error::Error;
use certificate_registry::CertificateRegistry;
use std::sync::mpsc::Sender;
use producer::ApnsResponse;

pub struct Consumer<'a> {
    channel: Channel,
    session: Session,
    notifiers: HashMap<String, Apns2Notifier>,
    control: Arc<AtomicBool>,
    metrics: Arc<Metrics<'a>>,
    config: Arc<Config>,
    certificate_registry: Arc<CertificateRegistry>,
    tx_response: Sender<ApnsResponse>,
}

impl<'a> Drop for Consumer<'a> {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl<'a> Consumer<'a> {
    pub fn new(control: Arc<AtomicBool>,
               metrics: Arc<Metrics<'a>>,
               config: Arc<Config>,
               certificate_registry: Arc<CertificateRegistry>,
               tx_response: Sender<(PushNotification, Option<AsyncResponse>)>) -> Consumer<'a> {

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
            notifiers: HashMap::new(),
            control: control,
            metrics: metrics,
            config: config,
            certificate_registry: certificate_registry,
            tx_response: tx_response,
        }
    }

    pub fn consume(&mut self) -> Result<(), Error> {
        while self.control.load(Ordering::Relaxed) {
            for result in self.channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    if let Some(notifier) = {
                        let application_id = event.get_application_id();

                        if !self.notifiers.contains_key(application_id) {
                            let create_notifier = move |cert: Cursor<&[u8]>, key: Cursor<&[u8]>| {
                                Apns2Notifier::new(cert, key)
                            };

                            match self.certificate_registry.with_certificate(application_id, create_notifier) {
                                Ok(notifier) => {
                                    self.notifiers.insert(application_id.to_string(), notifier);
                                },
                                Err(e) => {
                                    error!("Error when fetching notifier: {:?}", e);
                                }
                            }
                        }

                        self.notifiers.get(application_id)
                    } {
                        let response = notifier.send(&event);

                        self.metrics.gauges.in_flight.increment(1);
                        self.tx_response.send((event, Some(response))).unwrap();
                    } else {
                        self.metrics.gauges.in_flight.increment(1);
                        self.tx_response.send((event, None)).unwrap();
                    }
                }

                result.ack();

                if !self.control.load(Ordering::Relaxed) { break; }
            }

            thread::park_timeout(Duration::from_millis(100));
        }

        Ok(())
    }
}
