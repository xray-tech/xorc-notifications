use std::sync::Arc;
use std::time::{SystemTime, Duration};
use web_push::WebPushError;
use std::cmp;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::webpush_notification::WebPushResult;
use hyper::header::RetryAfter;
use config::Config;
use futures::sync::mpsc::Receiver;
use futures::Stream;
use protobuf::core::Message;
use std::sync::atomic::AtomicBool;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT};
use notifier::ProducerMessage;
use logger::GelfLogger;
use gelf::{Message as GelfMessage, Level as GelfLevel, Error as GelfError};

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

        let mut session = Session::new(options, control.clone()).unwrap();
        let mut channel = session.open_channel(1).unwrap();

        channel.exchange_declare(
            &*config.rabbitmq.response_exchange,
            &*config.rabbitmq.response_exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).unwrap();

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

    fn handle_no_cert(&mut self, mut event: PushNotification) {
        error!("Certificate missing for event: '{:?}'", event);
        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        event.mut_web().set_response(web_result);

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            "no_retry", // routing key
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().unwrap()).unwrap();
    }

    fn handle_error(&mut self, mut event: PushNotification, error: WebPushError) {
        let _ = self.log_result("Error sending a push notification", &event, Some(&error));

        let mut web_result = WebPushResult::new();
        web_result.set_successful(false);

        let routing_key = match error {
            WebPushError::ServerError(retry_after) => {
                let duration: u32 = match retry_after {
                    Some(RetryAfter::Delay(duration)) =>
                        cmp::max(0 as u64, duration.as_secs()) as u32,
                    Some(RetryAfter::DateTime(retry_time)) => {
                        let retry_system_time: SystemTime = retry_time.into();
                        let retry_duration = retry_system_time.duration_since(SystemTime::now()).unwrap_or(Duration::new(0, 0));
                        cmp::max(0 as u64, retry_duration.as_secs()) as u32
                    }
                    None => {
                        if event.has_retry_count() {
                            let base: u32 = 2;
                            base.pow(event.get_retry_count())
                        } else {
                            1
                        }
                    }
                };

                event.set_retry_after(duration);
                "retry"
            },
            WebPushError::TimeoutError => {
                let duration: u32 = if event.has_retry_count() {
                    let base: u32 = 2;
                    base.pow(event.get_retry_count())
                } else {
                    1
                };

                event.set_retry_after(duration);
                "retry"
            }
            WebPushError::Unauthorized => {
                CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();
                "no_retry"
            },
            WebPushError::Unspecified => {
                CALLBACKS_COUNTER.with_label_values(&["crypto_error"]).inc();
                "no_retry"
            },
            WebPushError::BadRequest => {
                CALLBACKS_COUNTER.with_label_values(&["bed_request"]).inc();
                "no_retry"
            },
            WebPushError::PayloadTooLarge => {
                CALLBACKS_COUNTER.with_label_values(&["payload_too_large"]).inc();
                "no_retry"
            },
            WebPushError::EndpointNotValid => {
                CALLBACKS_COUNTER.with_label_values(&["endpoint_not_valid"]).inc();
                "no_retry"
            },
            WebPushError::EndpointNotFound => {
                CALLBACKS_COUNTER.with_label_values(&["endpoint_not_found"]).inc();
                "no_retry"
            },
            WebPushError::InvalidUri => {
                CALLBACKS_COUNTER.with_label_values(&["invalid_uri"]).inc();
                "no_retry"
            },
            _ => {
                CALLBACKS_COUNTER.with_label_values(&["unknown"]).inc();
                "no_retry"
            },
        };

        event.mut_web().set_response(web_result);

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            routing_key, // routing key
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().unwrap()).unwrap();
    }

    fn handle_ok(&mut self, mut event: PushNotification) {
        let _ = self.log_result("Successfully sent a push notification", &event, None);
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
        let mut web_result = WebPushResult::new();
        web_result.set_successful(true);

        event.mut_web().set_response(web_result);

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            "no_retry", // routing key
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().unwrap()).unwrap();
    }

    fn log_result(&self, title: &str, event: &PushNotification, error: Option<&WebPushError>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

        match error {
            Some(error_msg) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", format!("{:?}", error_msg))?;
            },
            _ => {
                test_msg.set_metadata("successful", String::from("true"))?;
            }
        }

        self.logger.log_message(test_msg);

        Ok(())
    }
}
