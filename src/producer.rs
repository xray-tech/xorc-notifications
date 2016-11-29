use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use chrono::Duration;
use chrono::offset::utc::UTC;
use std::thread;
use std::cmp;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::google_notification::FcmResult;
use events::google_notification::FcmResult_Status::*;
use fcm::response::{FcmError, FcmResponse};
use config::Config;
use std::sync::mpsc::Receiver;
use protobuf::core::Message;
use retry_after::RetryAfter;
use metrics::CALLBACKS_COUNTER;

pub type FcmData = (PushNotification, Option<Result<FcmResponse, FcmError>>);

pub struct ResponseProducer {
    channel: Channel,
    session: Session,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    rx: Receiver<FcmData>,
}

impl Drop for ResponseProducer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl ResponseProducer {
    pub fn new(config: Arc<Config>, rx: Receiver<FcmData>, control: Arc<AtomicBool>) -> ResponseProducer {
        let options = Options {
            vhost: &config.rabbitmq.vhost,
            host: &config.rabbitmq.host,
            port: config.rabbitmq.port,
            login: &config.rabbitmq.login,
            password: &config.rabbitmq.password, .. Default::default()
        };

        let mut session = Session::new(options).unwrap();
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
            control: control,
            config: config.clone(),
            rx: rx,
        }
    }

    pub fn run(&mut self) {
        let wait_duration = Duration::milliseconds(100).to_std().unwrap();

        while self.control.load(Ordering::Relaxed) {
            match self.rx.try_recv() {
                Ok((mut event, Some(Ok(response)))) => {
                    let mut fcm_result = FcmResult::new();
                    let ref results = response.results.unwrap();
                    let ref result = results.first().unwrap();

                    if let Some(multicast_id) = response.multicast_id {
                        fcm_result.set_multicast_id(multicast_id);
                    }

                    if let Some(canonical_ids) = response.canonical_ids {
                        fcm_result.set_canonical_ids(canonical_ids);
                    }

                    if let Some(ref message_id) = result.message_id {
                        fcm_result.set_message_id(message_id.clone());
                    }

                    if let Some(ref registration_id) = result.registration_id {
                        fcm_result.set_registration_id(registration_id.clone());
                    }

                    if result.error.is_none() {
                        info!("Push notification result: '{:?}', event: '{:?}'", result, event);

                        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    } else {
                        error!("Error in sending push notification: '{:?}', event: '{:?}'", result, event);

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

                    event.mut_google().set_response(fcm_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "no_retry", // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();
                },
                Ok((mut event, Some(Err(error)))) => {
                    error!("Error in sending push notification: '{:?}', event: '{:?}'", &error, event);

                    let mut fcm_result = FcmResult::new();
                    fcm_result.set_successful(false);

                    let routing_key = match error {
                        FcmError::ServerError(retry_after) => {
                            fcm_result.set_status(ServerError);

                            let duration: u32 = match retry_after {
                                Some(RetryAfter::Delay(duration)) =>
                                    cmp::max(0 as i64, duration.num_seconds()) as u32,
                                Some(RetryAfter::DateTime(retry_time)) => {
                                    cmp::max(0 as i64, retry_time.timestamp() - UTC::now().timestamp()) as u32
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

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        routing_key, // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();
                },
                Ok((mut event, None)) => {
                    error!("Certificate missing for event: '{:?}'", event);
                    CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

                    let mut fcm_result = FcmResult::new();

                    fcm_result.set_successful(false);
                    fcm_result.set_status(MissingCertificate);

                    event.mut_google().set_response(fcm_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "no_retry", // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();
                },
                Err(_) => {
                    thread::park_timeout(wait_duration);
                },
            }
        }
    }
}
