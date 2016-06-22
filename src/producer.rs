use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use events::push_notification::PushNotification;
use events::google_notification::FcmResult;
use events::google_notification::FcmResult_Status::*;
use fcm::response::{FcmError, FcmResponse};
use config::Config;
use std::sync::mpsc::{channel, Receiver};
use protobuf::core::Message;
use metrics::Metrics;

pub type FcmData = (PushNotification, Result<FcmResponse, FcmError>);

pub struct ResponseProducer<'a> {
    channel: Channel,
    session: Session,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    rx: Receiver<FcmData>,
    metrics: Arc<Metrics<'a>>,
}

impl<'a> Drop for ResponseProducer<'a> {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl<'a> ResponseProducer<'a> {
    pub fn new(config: Arc<Config>, rx: Receiver<FcmData>, control: Arc<AtomicBool>,
               metrics: Arc<Metrics<'a>>) -> ResponseProducer<'a> {
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
            metrics: metrics,
        }
    }

    pub fn run(&mut self) {
        let wait_duration = Duration::from_millis(100);

        while self.control.load(Ordering::Relaxed) {
            match self.rx.try_recv() {
                Ok((mut event, Ok(response))) => {
                    let mut fcm_result = FcmResult::new();
                    let results = response.results.unwrap();
                    let result = results.first().unwrap();

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
                        self.metrics.counters.successful.increment(1);
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    } else {
                        self.metrics.counters.failure.increment(1);
                        fcm_result.set_successful(false);

                        let ref status = match result.error.as_ref().map(AsRef::as_ref) {
                            Some("InvalidTtl")                => InvalidTtl,
                            Some("Unavailable")               => Unavailable,
                            Some("MessageTooBig")             => MessageTooBig,
                            Some("NotRegistered")             => NotRegistered,
                            Some("InvalidDataKey")            => InvalidDataKey,
                            Some("MismatchSenderId")          => MismatchSenderId,
                            Some("InvalidPackageName")        => InvalidPackageName,
                            Some("MissingRegistration")       => MissingRegistration,
                            Some("InvalidRegistration")       => InvalidRegistration,
                            Some("DeviceMessageRateExceeded") => DeviceMessageRateExceeded,
                            Some("TopicsMessageRateExceeded") => TopicsMessageRateExceeded,
                            _                                 => Unknown,
                        };

                        fcm_result.set_status(*status);

                        event.mut_google().set_response(fcm_result);

                        self.channel.basic_publish(
                            &*self.config.rabbitmq.response_exchange,
                            "google", // routing key
                            false,   // mandatory
                            false,   // immediate
                            BasicProperties { ..Default::default() },
                            event.write_to_bytes().unwrap()).unwrap();
                    }
                },
                Ok((mut event, Err(error))) => {
                    self.metrics.counters.failure.increment(1);
                    let mut fcm_result = FcmResult::new();
                    fcm_result.set_successful(false);

                    match error {
                        FcmError::ServerError           => fcm_result.set_status(ServerError),
                        FcmError::Unauthorized          => fcm_result.set_status(Unauthorized),
                        FcmError::InvalidMessage(error) => {
                            fcm_result.set_status(InvalidMessage);
                            fcm_result.set_error(error);
                        },
                    }

                    event.mut_google().set_response(fcm_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "google", // routing key
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
