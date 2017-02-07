use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use events::push_notification::PushNotification;
use apns2::client::ProviderResponse;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use certificate_registry::CertificateRegistry;
use std::sync::mpsc::Sender;
use producer::ApnsResponse;
use pool::{ConnectionPool, ApnsConnection};
use metrics::CALLBACKS_INFLIGHT;

pub struct Consumer {
    channel: Channel,
    session: Session,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    pool: ConnectionPool,
    tx_response: Sender<ApnsResponse>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl Consumer {
    pub fn new(control: Arc<AtomicBool>,
               config: Arc<Config>,
               certificate_registry: Arc<CertificateRegistry>,
               tx_response: Sender<(PushNotification, Option<ProviderResponse>)>) -> Consumer {

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
            control: control,
            pool: ConnectionPool::new(certificate_registry, config.clone()),
            config: config,
            tx_response: tx_response,
        }
    }

    pub fn consume(&mut self) -> Result<(), Error> {
        while self.control.load(Ordering::Relaxed) {
            for result in self.channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    let response = match self.pool.get(event.get_application_id()) {
                        Some(ApnsConnection::WithCertificate { ref notifier, ref topic }) =>
                            Some(notifier.send(&event, topic)),
                        Some(ApnsConnection::WithToken { ref notifier, ref token, ref topic }) =>
                            Some(notifier.send(&event, topic, token.signature())),
                        None => None
                    };

                    CALLBACKS_INFLIGHT.inc();

                    self.tx_response.send((event, response)).unwrap();
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
