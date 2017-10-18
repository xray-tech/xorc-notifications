use std::sync::Arc;
use web_push::WebPushError;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::webpush_notification::WebPushResult;
use events::notification_result::NotificationResult;
use events::header::Header;
use config::Config;
use futures::sync::mpsc::Receiver;
use futures::Stream;
use protobuf::core::Message;
use std::sync::atomic::AtomicBool;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT};
use notifier::ProducerMessage;
use logger::GelfLogger;
use time;

pub struct ResponseProducer {
    channel: Channel,
    session: Session,
    config: Arc<Config>,
    logger: Arc<GelfLogger>,
}

impl Drop for ResponseProducer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl ResponseProducer {
    pub fn new(config: Arc<Config>, control: Arc<AtomicBool>, logger: Arc<GelfLogger>) -> ResponseProducer {
        let options = Options {
            vhost: config.rabbitmq.vhost.clone(),
            host: config.rabbitmq.host.clone(),
            port: config.rabbitmq.port,
            login: config.rabbitmq.login.clone(),
            password: config.rabbitmq.password.clone(), .. Default::default()
        };

        let mut session = Session::new(options, control.clone()).expect("Couldn't connect to RabbitMQ");
        let mut channel = session.open_channel(1).expect("Couldn't open a RabbitMQ channel");

        channel.exchange_declare(
            &*config.rabbitmq.response_exchange,
            &*config.rabbitmq.response_exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).expect("Couldn't declare a RabbitMQ exchange");

        ResponseProducer {
            channel: channel,
            session: session,
            config: config.clone(),
            logger: logger,
        }
    }

    pub fn run(&mut self, rx: Receiver<ProducerMessage>) {
        let mut iterator = rx.wait();

        while let Some(item) = iterator.next() {
            CALLBACKS_INFLIGHT.dec();

            match item {
                Ok((event, Some(Ok(())))) =>
                    self.handle_ok(event),
                Ok((event, Some(Err(error)))) =>
                    self.handle_error(event, error),
                Ok((event, None)) =>
                    self.handle_no_cert(event),
                Err(e) =>
                    error!("This should not happen! {:?}", e),
            }
        }
    }

    fn handle_ok(&mut self, mut event: PushNotification) {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let _ = self.logger.log_push_result("Successfully sent a push notification", &event, None);
        let mut web_result = WebPushResult::new();

        web_result.set_successful(true);
        event.mut_web().set_response(web_result);

        self.publish(event, "no_retry");
    }

    fn handle_no_cert(&mut self, mut event: PushNotification) {
        error!("Certificate missing for event: '{:?}'", event);
        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        event.mut_web().set_response(web_result);

        self.publish(event, "no_retry");
    }

    fn handle_error(&mut self, mut event: PushNotification, error: WebPushError) {
        let _ = self.logger.log_push_result("Error sending a push notification", &event, Some(&error));

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        web_result.set_error((&error).into());

        event.mut_web().set_response(web_result);

        CALLBACKS_COUNTER.with_label_values(&[error.short_description()]).inc();

        match error {
            WebPushError::ServerError(retry_after) => {
                match retry_after {
                    Some(duration) =>
                        event.set_retry_after(duration.as_secs() as u32),
                    None => {
                        let duration = Self::calculate_retry_duration(&event);
                        event.set_retry_after(duration)
                    }
                }

                self.publish(event, "retry");
            },
            WebPushError::TimeoutError => {
                let duration = Self::calculate_retry_duration(&event);

                event.set_retry_after(duration);

                self.publish(event, "retry");
            },
            _ => {
                self.publish(event, "no_retry");
            },
        };
    }

    fn can_reply(event: &PushNotification, routing_key: &str) -> bool {
        event.has_exchange() && routing_key == "no_retry" && event.has_response_recipient_id()
    }

    fn publish(&mut self, event: PushNotification, routing_key: &str) {
        if Self::can_reply(&event, routing_key) {
            let response_routing_key = event.get_response_recipient_id();
            let response = event.get_web().get_response();
            let current_time         = time::get_time();
            let mut header           = Header::new();

            header.set_created_at((current_time.sec as i64 * 1000) +
                                  (current_time.nsec as i64 / 1000 / 1000));
            header.set_source(String::from("webpush"));
            header.set_recipient_id(String::from(response_routing_key));
            header.set_field_type(String::from("notification.NotificationResult"));

            let mut result_event = NotificationResult::new();
            result_event.set_header(header);
            result_event.set_universe(String::from(event.get_universe()));
            result_event.set_correlation_id(String::from(event.get_correlation_id()));

            if response.has_error() {
                result_event.set_successful(false);
                result_event.set_error((&response.get_error()).into());
                result_event.set_reason(format!("{:?}", response.get_error()));
            } else {
                result_event.set_successful(true);
            }

            self.channel.basic_publish(
                event.get_exchange(),
                response_routing_key,
                false,   // mandatory
                false,   // immediate
                BasicProperties { ..Default::default() },
                result_event.write_to_bytes().expect("Couldn't serialize a protobuf event")).expect("Couldn't publish to RabbitMQ");
        }

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            routing_key,
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().expect("Couldn't serialize a protobuf event")).expect("Couldn't publish to RabbitMQ");
    }

    fn calculate_retry_duration(event: &PushNotification) -> u32 {
        if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        }
    }
}
