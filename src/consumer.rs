use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use std::thread::park_timeout;
use amqp::{Session, Channel, Table, Basic, Options, Consumer as AmqpConsumer};
use amqp::protocol::basic;
use events::push_notification::PushNotification;
use apns2::client::ProviderResponse;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use certificate_registry::CertificateRegistry;
use std::sync::mpsc::Sender;
use pool::{ConnectionPool, ApnsConnection};
use metrics::CALLBACKS_INFLIGHT;

pub struct Consumer {
    channel: Channel,
    session: Session,
    config: Arc<Config>,
    certificate_registry: Arc<CertificateRegistry>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl Consumer {
    pub fn new(control: Arc<AtomicBool>, config: Arc<Config>, certificate_registry: Arc<CertificateRegistry>) -> Consumer {
        let mut session = Session::new(Options {
            vhost: config.rabbitmq.vhost.clone(),
            host: config.rabbitmq.host.clone(),
            port: config.rabbitmq.port,
            login: config.rabbitmq.login.clone(),
            password: config.rabbitmq.password.clone(), .. Default::default()
        }, control.clone()).expect("Couldn't connect to RabbitMQ");

        let mut channel = session.open_channel(1).expect("Couldn't open a RabbitMQ channel");

        channel.queue_declare(
            &*config.rabbitmq.queue,
            false, // passive
            true,  // durable
            false, // exclusive
            false, // auto_delete
            false, // nowait
            Table::new()).expect("Couldn't declare a RabbitMQ queue");

        channel.exchange_declare(
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).expect("Couldn't declare a RabbitMQ exchange");

        channel.queue_bind(
            &*config.rabbitmq.queue,
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.routing_key,
            false, // nowait
            Table::new()).expect("Couldn't bind a RabbitMQ queue to an exchange");

        Consumer {
            channel: channel,
            session: session,
            config: config,
            certificate_registry: certificate_registry,
        }
    }

    pub fn consume(&mut self, tx_response: Sender<(PushNotification, Option<ProviderResponse>)>) -> Result<(), Error> {
        let consumer = ApnsConsumer::new(self.certificate_registry.clone(), self.config.clone(), tx_response);

        self.channel.basic_prefetch(100).ok().expect("failed to prefetch");

        let consumer_name = self.channel.basic_consume(consumer,
                                                       &*self.config.rabbitmq.queue,
                                                       "apns_consumer",
                                                       true,  // no local
                                                       false, // no ack
                                                       false, // exclusive
                                                       false, // nowait
                                                       Table::new());

        info!("Starting consumer {:?}", consumer_name);
        self.channel.start_consuming();

        Ok(())
    }
}

struct ApnsConsumer {
    pool: ConnectionPool,
    tx_response: Sender<(PushNotification, Option<ProviderResponse>)>,
}

impl ApnsConsumer {
    pub fn new(registry: Arc<CertificateRegistry>, config: Arc<Config>,
               tx_response: Sender<(PushNotification, Option<ProviderResponse>)>) -> ApnsConsumer {
        ApnsConsumer {
            pool: ConnectionPool::new(registry, config),
            tx_response: tx_response,
        }
    }
}

impl AmqpConsumer for ApnsConsumer {
    fn handle_delivery(&mut self, channel: &mut Channel, deliver: basic::Deliver,
                       _headers: basic::BasicProperties, body: Vec<u8>) {
        if CALLBACKS_INFLIGHT.get() < 10000.0 {
            channel.basic_ack(deliver.delivery_tag, false).unwrap();

            if let Ok(event) = parse_from_bytes::<PushNotification>(&body) {
                let response = match self.pool.get(event.get_application_id()) {
                    Some(ApnsConnection::WithCertificate { ref notifier, ref topic }) =>
                        Some(notifier.send(&event, topic)),
                    Some(ApnsConnection::WithToken { ref notifier, ref token, ref topic }) => {
                        let signature = token.signature();

                        println!("App: {}, Topic: {}, Signature: {}", event.get_application_id(), topic, signature);

                        Some(notifier.send(&event, topic, signature))
                    },
                    None => None
                };

                CALLBACKS_INFLIGHT.inc();

                self.tx_response.send((event, response)).unwrap();
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
