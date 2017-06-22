use std::sync::Arc;
use std::time::{SystemTime, Duration};
use std::cmp;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::google_notification::FcmResult;
use events::google_notification::FcmResult_Status::*;
use hyper::header::RetryAfter;
use fcm::response::{FcmError, FcmResponse};
use config::Config;
use futures::sync::mpsc::Receiver;
use futures::Stream;
use protobuf::core::Message;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT};
use std::sync::atomic::AtomicBool;

pub type FcmData = (PushNotification, Option<Result<FcmResponse, FcmError>>);

pub struct ResponseProducer {
    channel: Channel,
    session: Session,
    config: Arc<Config>,
}

impl Drop for ResponseProducer {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl ResponseProducer {
    pub fn new(config: Arc<Config>, control: Arc<AtomicBool>) -> ResponseProducer {
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

    fn handle_no_cert(&mut self, mut event: PushNotification) {
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
    }

    fn handle_error(&mut self, mut event: PushNotification, error: FcmError) {
        error!("Error in sending push notification: '{:?}', event: '{:?}'", &error, event);

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

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            routing_key, // routing key
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().unwrap()).unwrap();
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
                },
                None => {
                    CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                    fcm_result.set_successful(false);
                    fcm_result.set_status(Unknown);
                }
            },
            None => {
                CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                fcm_result.set_successful(false);
                fcm_result.set_status(Unknown);
            }
        }

        event.mut_google().set_response(fcm_result);

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            "no_retry", // routing key
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().unwrap()).unwrap();
    }

}
