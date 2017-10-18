use std::sync::Arc;
use std::time::{SystemTime, Duration};
use std::cmp;
use time;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::notification_result::{NotificationResult, NotificationResult_Error};
use events::google_notification::FcmResult;
use events::google_notification::FcmResult_Status::*;
use events::header::Header;
use hyper::header::RetryAfter;
use fcm::response::{FcmError, FcmResponse};
use config::Config;
use futures::sync::mpsc::Receiver;
use futures::Stream;
use protobuf::core::Message;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT};
use std::sync::atomic::AtomicBool;
use logger::GelfLogger;
use gelf::{Message as GelfMessage, Error as GelfError, Level as GelfLevel};

pub type FcmData = (PushNotification, Option<Result<FcmResponse, FcmError>>);

pub struct ResponseProducer {
    channel: Channel,
    session: Session,
    config: Arc<Config>,
    logger: Arc<GelfLogger>
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

    pub fn run(&mut self, rx: Receiver<FcmData>) {
        let mut iterator = rx.wait();

        while let Some(item) = iterator.next() {
            CALLBACKS_INFLIGHT.dec();

            match item {
                Ok((event, Some(Ok(response)))) =>
                    self.handle_response(event, response),
                Ok((event, Some(Err(error)))) =>
                    self.handle_error(event, error),
                Ok((event, None)) =>
                    self.handle_no_cert(event),
                Err(e) =>
                    error!("This should not happen! {:?}", e),
            }
        }
    }

    fn can_reply(event: &PushNotification, routing_key: &str) -> bool {
        event.has_exchange() && routing_key == "no_retry" && event.has_response_recipient_id()
    }

    fn publish(&mut self, event: PushNotification, routing_key: &str) {
        if Self::can_reply(&event, routing_key) {
            let response_routing_key = event.get_response_recipient_id();
            let response             = event.get_google().get_response();
            let current_time         = time::get_time();
            let mut header           = Header::new();

            header.set_created_at((current_time.sec as i64 * 1000) +
                                  (current_time.nsec as i64 / 1000 / 1000));
            header.set_source(String::from("fcm"));
            header.set_recipient_id(String::from(response_routing_key));
            header.set_field_type(String::from("notification.NotificationResult"));

            let mut result_event = NotificationResult::new();
            result_event.set_header(header);
            result_event.set_universe(String::from(event.get_universe()));
            result_event.set_correlation_id(String::from(event.get_correlation_id()));

            match response.get_status() {
                Success => {
                    result_event.set_delete_user(false);
                    result_event.set_successful(true);
                },
                NotRegistered => {
                    result_event.set_delete_user(true);
                    result_event.set_successful(false);
                    result_event.set_error(NotificationResult_Error::Unsubscribed);
                },
                _ => {
                    result_event.set_delete_user(false);
                    result_event.set_successful(false);
                    result_event.set_reason(format!("{:?}", response.get_status()));
                    result_event.set_error(NotificationResult_Error::Other);
                },
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

    fn handle_no_cert(&mut self, mut event: PushNotification) {
        let _ = self.log_result("Error sending a push notification", &event, Some("MissingCertificateOrToken"));

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut fcm_result = FcmResult::new();

        fcm_result.set_successful(false);
        fcm_result.set_status(MissingCertificate);

        event.mut_google().set_response(fcm_result);

        self.publish(event, "no_retry");
    }

    fn handle_error(&mut self, mut event: PushNotification, error: FcmError) {
        let error_str = format!("{:?}", error);
        let _ = self.log_result("Error sending a push notification", &event, Some(&error_str));

        let mut fcm_result = FcmResult::new();
        fcm_result.set_successful(false);

        let routing_key = match error {
            FcmError::ServerError(retry_after) => {
                fcm_result.set_status(ServerError);

                let duration: u32 = match retry_after {
                    Some(RetryAfter::Delay(duration)) =>
                        cmp::max(0, duration.as_secs()) as u32,
                    Some(RetryAfter::DateTime(retry_time)) => {
                        let retry_system_time: SystemTime = retry_time.into();
                        let retry_duration = retry_system_time.duration_since(SystemTime::now()).unwrap_or(Duration::new(0, 0));

                        cmp::max(0, retry_duration.as_secs()) as u32
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

                CALLBACKS_COUNTER.with_label_values(&["server_error"]).inc();

                event.set_retry_after(duration);
                "retry"
            },
            FcmError::Unauthorized             => {
                fcm_result.set_status(Unauthorized);
                CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();
                "no_retry"
            },
            FcmError::InvalidMessage(error)    => {
                fcm_result.set_status(InvalidMessage);
                CALLBACKS_COUNTER.with_label_values(&["invalid_message"]).inc();
                fcm_result.set_error(error);
                "no_retry"
            },
        };

        event.mut_google().set_response(fcm_result);

        self.publish(event, routing_key);
    }

    fn log_result(&self, title: &str, event: &PushNotification, error: Option<&str>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

        if let Some(msg) = error {
            test_msg.set_metadata("successful", String::from("false"))?;
            test_msg.set_metadata("error", format!("{}", msg))?;
        } else {
            test_msg.set_metadata("successful", String::from("true"))?;
        }

        self.logger.log_message(test_msg);

        Ok(())
    }

    fn handle_response(&mut self, mut event: PushNotification, response: FcmResponse) {
        let mut fcm_result = FcmResult::new();

        if let Some(multicast_id) = response.multicast_id {
            fcm_result.set_multicast_id(multicast_id);
        }

        if let Some(canonical_ids) = response.canonical_ids {
            fcm_result.set_canonical_ids(canonical_ids);
        }

        match response.results {
            Some(results) => match results.first() {
                Some(result) => {
                    if let Some(ref message_id) = result.message_id {
                        fcm_result.set_message_id(message_id.clone());
                    }

                    if let Some(ref registration_id) = result.registration_id {
                        fcm_result.set_registration_id(registration_id.clone());
                    }

                    if result.error.is_none() {
                        let _ = self.log_result("Successfully sent a push notification", &event, None);

                        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    } else {
                        let _ = self.log_result("Error sending a push notification", &event, result.error.as_ref().map(AsRef::as_ref));

                        fcm_result.set_successful(false);

                        let (status, status_str) = match result.error.as_ref().map(AsRef::as_ref) {
                            Some("InvalidTtl")                => (InvalidTtl, "invalid_ttl"),
                            Some("Unavailable")               => (Unavailable, "unavailable"),
                            Some("MessageTooBig")             => (MessageTooBig, "message_too_big"),
                            Some("NotRegistered")             => (NotRegistered, "not_registered"),
                            Some("InvalidDataKey")            => (InvalidDataKey, "invalid_data_key"),
                            Some("MismatchSenderId")          => (MismatchSenderId, "mismatch_sender_id"),
                            Some("InvalidPackageName")        => (InvalidPackageName, "invalid_package_name"),
                            Some("MissingRegistration")       => (MissingRegistration, "missing_registration"),
                            Some("InvalidRegistration")       => (InvalidRegistration, "invalid_registration"),
                            Some("DeviceMessageRateExceeded") => (DeviceMessageRateExceeded, "device_message_rate_exceeded"),
                            Some("TopicsMessageRateExceeded") => (TopicsMessageRateExceeded, "topics_message_rate_exceeded"),
                            _                                 => (Unknown, "unknown_error"),
                        };

                        CALLBACKS_COUNTER.with_label_values(&[status_str]).inc();

                        fcm_result.set_status(status);
                    }
                },
                None => {
                    let _ = self.log_result("Error sending a push notification", &event, Some("UnknownError"));

                    CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                    fcm_result.set_successful(false);
                    fcm_result.set_status(Unknown);
                }
            },
            None => {
                let _ = self.log_result("Error sending a push notification", &event, Some("UnknownError"));

                CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                fcm_result.set_successful(false);
                fcm_result.set_status(Unknown);
            }
        }

        event.mut_google().set_response(fcm_result);

        self.publish(event, "no_retry");
    }
}
