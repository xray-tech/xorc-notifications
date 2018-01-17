use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, Duration};
use amqp::{Session, Channel, Table, Basic, Options, Consumer as AmqpConsumer, AMQPError};
use amqp::protocol::basic;
use events::push_notification::PushNotification;
use protobuf::parse_from_bytes;
use config::Config;
use std::sync::mpsc::Sender;
use consumer_supervisor::{ApnsConnection, CONSUMER_FAILURES};
use metrics::REQUEST_COUNTER;
use producer::ApnsResponse;
use logger::{GelfLogger, LogAction};
use gelf::{Message as GelfMessage, Level as GelfLevel};

pub struct Consumer {
    channel: Channel,
    session: Session,
    queue: String,
    control: Arc<AtomicBool>,
    app_id: i32,
    logger: Arc<GelfLogger>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl Consumer {
    pub fn new(control: Arc<AtomicBool>, config: Arc<Config>, logger: Arc<GelfLogger>, app_id: i32) -> Consumer {
        let mut session = Session::new(Options {
            vhost: config.rabbitmq.vhost.clone(),
            host: config.rabbitmq.host.clone(),
            port: config.rabbitmq.port,
            login: config.rabbitmq.login.clone(),
            password: config.rabbitmq.password.clone(), .. Default::default()
        }, control.clone()).expect("Couldn't connect to RabbitMQ");

        let mut channel = session.open_channel(1).expect("Couldn't open a RabbitMQ channel");
        let queue_name = format!("{}_{}", config.rabbitmq.queue, app_id);
        let routing_key = format!("{}_{}", config.rabbitmq.routing_key, app_id);

        channel.queue_declare(
            &*queue_name,
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
            &*queue_name,
            &*config.rabbitmq.exchange,
            &*routing_key,
            false, // nowait
            Table::new()).expect("Couldn't bind a RabbitMQ queue to an exchange");

        info!("Adding a consumer to queue {}", &queue_name);

        Consumer {
            control: control,
            channel: channel,
            session: session,
            queue: queue_name,
            app_id: app_id,
            logger: logger,
        }
    }

    pub fn consume(&mut self, apns: ApnsConnection, tx_response: Sender<ApnsResponse>) -> Result<(), AMQPError> {
        let consumer = ApnsConsumer::new(apns, tx_response, self.control.clone(), self.logger.clone(), self.app_id);

        self.channel.basic_prefetch(100).ok().expect("failed to prefetch");

        let consumer_name = self.channel.basic_consume(consumer,
                                                       &*self.queue,
                                                       "apns_consumer",
                                                       true,  // no local
                                                       false, // no ack
                                                       false, // exclusive
                                                       false, // nowait
                                                       Table::new())?;

        let mut log_msg = GelfMessage::new(String::from("Consumer started"));

        log_msg.set_full_message(String::from("Consumer is created and will now start consuming messages"));
        log_msg.set_level(GelfLevel::Informational);

        let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerStart));
        let _ = log_msg.set_metadata("app_id", format!("{}", self.app_id));
        let _ = log_msg.set_metadata("consumer_name", format!("{}", consumer_name));
        let _ = log_msg.set_metadata("queue", format!("{}", self.queue));

        self.logger.log_message(log_msg);
        self.channel.start_consuming();

        Ok(())
    }
}

struct ApnsConsumer {
    connection: ApnsConnection,
    tx_response: Sender<ApnsResponse>,
    last_ping: SystemTime,
    control: Arc<AtomicBool>,
    app_id: i32,
    logger: Arc<GelfLogger>,
}

impl ApnsConsumer {
    pub fn new(connection: ApnsConnection,
               tx_response: Sender<ApnsResponse>,
               control: Arc<AtomicBool>,
               logger: Arc<GelfLogger>,
               app_id: i32) -> ApnsConsumer {

        ApnsConsumer {
            connection: connection,
            tx_response: tx_response,
            last_ping: SystemTime::now(),
            control: control,
            logger: logger,
            app_id: app_id,
        }
    }

    fn connection_status(&mut self) -> Result<(), &'static str> {
        match SystemTime::now().duration_since(self.last_ping) {
            Ok(duration) if duration > Duration::from_secs(120) => {
                let mut log_msg = GelfMessage::new(String::from("Consumer connection health check"));
                let _ = log_msg.set_metadata("app_id", format!("{}", self.app_id));
                let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerHealthCheck));

                log_msg.set_full_message(String::from("Sending an HTTP2 ping to check the connection health"));

                self.last_ping = SystemTime::now();

                let result = match self.connection {
                    ApnsConnection::Certificate { notifier: ref n, topic: _ } =>
                        n.client.client.ping(),
                    ApnsConnection::Token { notifier: ref n, token: _, topic: _ } =>
                        n.client.client.ping(),
                };

                match result {
                    Ok(_) => {
                        let _ = log_msg.set_metadata("successful", String::from("true"));
                        log_msg.set_level(GelfLevel::Informational);
                    },
                    Err(e) => {
                        let _ = log_msg.set_metadata("successful", String::from("false"));
                        let _ = log_msg.set_metadata("error", format!("{:?}", e));
                        log_msg.set_level(GelfLevel::Error);
                    }
                }

                self.logger.log_message(log_msg);

                result
            },
            _ => Ok(())
        }
    }
}

impl AmqpConsumer for ApnsConsumer {
    fn handle_delivery(&mut self, channel: &mut Channel, deliver: basic::Deliver,
                       _headers: basic::BasicProperties, body: Vec<u8>) {
        match self.connection_status() {
            Ok(_) => {
                channel.basic_ack(deliver.delivery_tag, false).unwrap();

                if let Ok(event) = parse_from_bytes::<PushNotification>(&body) {
                    REQUEST_COUNTER.with_label_values(&["requested",
                                                        event.get_application_id(),
                                                        event.get_campaign_id()]).inc();

                    let response = match self.connection {
                        ApnsConnection::Certificate { ref notifier, ref topic } => {
                            Some(notifier.send(&event, topic))
                        },
                        ApnsConnection::Token { ref notifier, ref mut token, ref topic } => {
                            if token.is_expired() { token.renew().unwrap(); }
                            let signature = token.signature();

                            Some(notifier.send(&event, topic, signature))
                        }
                    };

                    self.tx_response.send((event, response)).unwrap();
                } else {
                    let mut log_msg = GelfMessage::new(String::from("Broken protobuf data"));
                    let _ = log_msg.set_metadata("app_id", format!("{}", self.app_id));

                    log_msg.set_full_message(String::from("The input data couldn't be loaded. Not valid protobuf."));
                    log_msg.set_level(GelfLevel::Error);

                    self.logger.log_message(log_msg);
                }
            },
            Err(_) => {
                channel.basic_nack(deliver.delivery_tag, false, true).unwrap();

                self.control.store(false, Ordering::Relaxed);
                let mut failures = CONSUMER_FAILURES.lock().unwrap();

                failures.insert(self.app_id);
            }
        }
    }
}
