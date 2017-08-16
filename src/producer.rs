use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::thread;
use events::push_notification::PushNotification;
use events::apple_notification::ApnsResult;
use events::apple_notification::ApnsResult_Status;
use events::apple_notification::ApnsResult_Status::*;
use events::apple_notification::ApnsResult_Reason;
use events::apple_notification::ApnsResult_Reason::*;
use time::precise_time_ns;
use config::Config;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use apns2::client::{ProviderResponse, Response, APNSStatus, APNSError};
use protobuf::core::Message;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT, RESPONSE_TIMES_HISTOGRAM};
use logger::GelfLogger;
use gelf::{Message as GelfMessage, Level as GelfLevel, Error as GelfError};

pub type ApnsResponse = (PushNotification, Option<ProviderResponse>);

pub struct ResponseProducer {
    config: Arc<Config>,
    session: Session,
    channel: Channel,
    rx: Receiver<(PushNotification, Option<ProviderResponse>)>,
    control: Arc<AtomicBool>,
    logger: Arc<GelfLogger>
}

impl Drop for ResponseProducer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl ResponseProducer {
    pub fn new(config: Arc<Config>, rx: Receiver<ApnsResponse>, control: Arc<AtomicBool>, logger: Arc<GelfLogger>) -> ResponseProducer {
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
            config: config.clone(),
            session: session,
            channel: channel,
            rx: rx,
            control: control,
            logger: logger,
        }
    }

    pub fn run(&mut self) {
        let wait_duration     = Duration::from_millis(100);
        let response_timeout  = Duration::new(2, 0);

        while self.is_running() {
            match self.rx.try_recv() {
                Ok((mut event, Some(async_response))) => {
                    let mut apns_result        = ApnsResult::new();
                    let response               = async_response.recv_timeout(response_timeout);
                    let response_time          = precise_time_ns() - async_response.requested_at;

                    RESPONSE_TIMES_HISTOGRAM.observe((response_time as f64) / 1000000000.0);

                    let routing_key = match response {
                        Ok(result) => {
                            let _ = self.log_result("Successfully sent a push notification", &event, Some(&result));

                            apns_result.set_successful(true);
                            apns_result.set_status(Self::convert_status(&result.status));

                            match Self::convert_reason_to_string(&result.reason) {
                                Some(reason) => CALLBACKS_COUNTER.with_label_values(&[&reason]).inc(),
                                None => CALLBACKS_COUNTER.with_label_values(&[&Self::convert_status_to_string(&result.status)]).inc(),
                            }

                            "no_retry"
                        },
                        Err(result) => {
                            apns_result.set_status(Self::convert_status(&result.status));
                            apns_result.set_successful(false);

                            if let Some(reason) = Self::convert_reason(&result.reason) {
                                apns_result.set_reason(reason);
                            }

                            let retry_after = if event.has_retry_count() {
                                let base: u32 = 2;
                                base.pow(event.get_retry_count())
                            } else {
                                1
                            };

                            if let Some(ts) = result.timestamp {
                                apns_result.set_timestamp(ts.to_timespec().sec);
                            }

                            match Self::convert_reason_to_string(&result.reason) {
                                Some(reason) => CALLBACKS_COUNTER.with_label_values(&[&reason]).inc(),
                                None => CALLBACKS_COUNTER.with_label_values(&[&Self::convert_status_to_string(&result.status)]).inc(),
                            }

                            match apns_result.get_reason() {
                                InternalServerError | Shutdown | ServiceUnavailable | ExpiredProviderToken => {
                                    let _ = self.log_result("Retrying a push notification", &event, Some(&result));

                                    event.set_retry_after(retry_after);
                                    "retry"
                                },
                                _ => match apns_result.get_status() {
                                    Timeout | Unknown | MissingChannel | Forbidden => {
                                        let _ = self.log_result("Retrying a push notification", &event, Some(&result));

                                        event.set_retry_after(retry_after);
                                        "retry"
                                    },
                                    _ => {
                                        let _ = self.log_result("Error sending a push notification", &event, Some(&result));
                                        "no_retry"
                                    },
                                }
                            }
                        }
                    };

                    event.mut_apple().set_result(apns_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        routing_key, // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();

                    CALLBACKS_INFLIGHT.dec();
                },
                Ok((mut event, None)) => {
                    let _ = self.log_result("Error sending a push notification", &event, None);

                    let mut apns_result = ApnsResult::new();

                    apns_result.set_successful(false);
                    apns_result.set_status(Error);
                    apns_result.set_reason(MissingCertificate);

                    event.mut_apple().set_result(apns_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "no_retry", // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();

                    CALLBACKS_INFLIGHT.dec();
                    CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();
                },
                Err(_) => {
                    thread::park_timeout(wait_duration);
                },
            }
        }
    }

    fn is_running(&self) -> bool {
        self.control.load(Ordering::Relaxed) || CALLBACKS_INFLIGHT.get() > 0.0
    }

    fn convert_status(status: &APNSStatus) -> ApnsResult_Status {
        match *status {
            APNSStatus::Success          => Success,
            APNSStatus::BadRequest       => BadRequest,
            APNSStatus::MissingChannel   => MissingChannel,
            APNSStatus::Timeout          => Timeout,
            APNSStatus::Unknown          => Unknown,
            APNSStatus::Unregistered     => Unregistered,
            APNSStatus::Forbidden        => Forbidden,
            APNSStatus::MethodNotAllowed => MethodNotAllowed,
            APNSStatus::PayloadTooLarge  => PayloadTooLarge,
            APNSStatus::TooManyRequests  => TooManyRequests,
            _                            => Error,
        }
    }

    fn convert_status_to_string(status: &APNSStatus) -> String {
        match *status {
            APNSStatus::Success          => String::from("success"),
            APNSStatus::BadRequest       => String::from("bad_request"),
            APNSStatus::MissingChannel   => String::from("missing_channel"),
            APNSStatus::Timeout          => String::from("timeout"),
            APNSStatus::Unknown          => String::from("unknown"),
            APNSStatus::Unregistered     => String::from("unregistered"),
            APNSStatus::Forbidden        => String::from("forbidden"),
            APNSStatus::MethodNotAllowed => String::from("method_not_allowed"),
            APNSStatus::PayloadTooLarge  => String::from("payload_too_large"),
            APNSStatus::TooManyRequests  => String::from("too_many_requests"),
            _                            => String::from("error"),
        }
    }

    fn convert_reason_to_string(reason: &Option<APNSError>) -> Option<String> {
        match *reason {
            Some(APNSError::PayloadEmpty)              => Some(String::from("payload_empty")),
            Some(APNSError::PayloadTooLarge)           => Some(String::from("payload_too_large")),
            Some(APNSError::BadTopic)                  => Some(String::from("bad_topic")),
            Some(APNSError::TopicDisallowed)           => Some(String::from("topic_disallowed")),
            Some(APNSError::BadMessageId)              => Some(String::from("bad_message_id")),
            Some(APNSError::BadExpirationDate)         => Some(String::from("bad_expiration_date")),
            Some(APNSError::BadPriority)               => Some(String::from("bad_priority")),
            Some(APNSError::MissingDeviceToken)        => Some(String::from("missing_device_token")),
            Some(APNSError::BadDeviceToken)            => Some(String::from("bad_device_token")),
            Some(APNSError::DeviceTokenNotForTopic)    => Some(String::from("device_token_not_for_topic")),
            Some(APNSError::Unregistered)              => Some(String::from("unregistered")),
            Some(APNSError::DuplicateHeaders)          => Some(String::from("duplicate_headers")),
            Some(APNSError::BadCertificateEnvironment) => Some(String::from("bad_certificate_environment")),
            Some(APNSError::BadCertificate)            => Some(String::from("bad_certificate")),
            Some(APNSError::Forbidden)                 => Some(String::from("forbidden")),
            Some(APNSError::BadPath)                   => Some(String::from("bad_path")),
            Some(APNSError::MethodNotAllowed)          => Some(String::from("method_not_allowed")),
            Some(APNSError::TooManyRequests)           => Some(String::from("too_many_requests")),
            Some(APNSError::IdleTimeout)               => Some(String::from("idle_timeout")),
            Some(APNSError::Shutdown)                  => Some(String::from("shutdown")),
            Some(APNSError::InternalServerError)       => Some(String::from("internal_server_error")),
            Some(APNSError::ServiceUnavailable)        => Some(String::from("service_unavailable")),
            Some(APNSError::MissingTopic)              => Some(String::from("missing_topic")),
            Some(APNSError::InvalidProviderToken)      => Some(String::from("invalid_provider_token")),
            Some(APNSError::MissingProviderToken)      => Some(String::from("missing_provider_token")),
            Some(APNSError::ExpiredProviderToken)      => Some(String::from("expired_provider_token")),
            _                                          => None,
        }
    }

    fn convert_reason(reason: &Option<APNSError>) -> Option<ApnsResult_Reason> {
        match *reason {
            Some(APNSError::PayloadEmpty)              => Some(PayloadEmpty),
            Some(APNSError::BadTopic)                  => Some(BadTopic),
            Some(APNSError::TopicDisallowed)           => Some(TopicDisallowed),
            Some(APNSError::BadMessageId)              => Some(BadMessageId),
            Some(APNSError::BadExpirationDate)         => Some(BadExpirationDate),
            Some(APNSError::BadPriority)               => Some(BadPriority),
            Some(APNSError::MissingDeviceToken)        => Some(MissingDeviceToken),
            Some(APNSError::BadDeviceToken)            => Some(BadDeviceToken),
            Some(APNSError::DeviceTokenNotForTopic)    => Some(DeviceTokenNotForTopic),
            Some(APNSError::DuplicateHeaders)          => Some(DuplicateHeaders),
            Some(APNSError::BadCertificateEnvironment) => Some(BadCertificateEnvironment),
            Some(APNSError::BadCertificate)            => Some(BadCertificate),
            Some(APNSError::BadPath)                   => Some(BadPath),
            Some(APNSError::IdleTimeout)               => Some(IdleTimeout),
            Some(APNSError::Shutdown)                  => Some(Shutdown),
            Some(APNSError::InternalServerError)       => Some(InternalServerError),
            Some(APNSError::ServiceUnavailable)        => Some(ServiceUnavailable),
            Some(APNSError::MissingTopic)              => Some(MissingTopic),
            Some(APNSError::InvalidProviderToken)      => Some(InvalidProviderToken),
            Some(APNSError::MissingProviderToken)      => Some(MissingProviderToken),
            Some(APNSError::ExpiredProviderToken)      => Some(ExpiredProviderToken),
            _                                          => None,
        }
    }

    fn log_result(&self, title: &str, event: &PushNotification, response: Option<&Response>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

        if let Some(r) = response {
            match r.status {
                APNSStatus::Success => {
                    test_msg.set_metadata("successful", String::from("true"))?;
                },
                ref status => {
                    test_msg.set_metadata("successful", String::from("false"))?;
                    test_msg.set_metadata("error", format!("{:?}", status))?;

                    if let Some(ref reason) = r.reason {
                        test_msg.set_metadata("reason", format!("{:?}", reason))?;
                    }
                }
            }
        } else {
            test_msg.set_metadata("successful", String::from("false"))?;
            test_msg.set_metadata("error", String::from("MissingCertificateOrToken"))?;
        }

        self.logger.log_message(test_msg);

        Ok(())
    }
}
