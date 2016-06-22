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
use apns2::{AsyncResponse, APNSStatus, APNSError};
use protobuf::core::Message;
use metrics::Metrics;

pub type ApnsResponse = (PushNotification, Option<AsyncResponse>);

pub struct ResponseProducer<'a> {
    config: Arc<Config>,
    session: Session,
    channel: Channel,
    rx: Receiver<(PushNotification, Option<AsyncResponse>)>,
    control: Arc<AtomicBool>,
    metrics: Arc<Metrics<'a>>,
}

impl<'a> Drop for ResponseProducer<'a> {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl<'a> ResponseProducer<'a> {
    pub fn new(config: Arc<Config>, rx: Receiver<ApnsResponse>,
               control: Arc<AtomicBool>, metrics: Arc<Metrics<'a>>) -> ResponseProducer<'a> {

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
            config: config.clone(),
            session: session,
            channel: channel,
            rx: rx,
            control: control,
            metrics: metrics,
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

                    self.metrics.timers.response_time.record(response_time);

                    match response {
                        Ok(result) => {
                            info!("Sent push notification for {}: {:?} ({} ms)",
                                event.get_application_id(), result, response_time / 1000000);

                            apns_result.set_successful(true);
                            apns_result.set_status(Self::convert_status(result.status));

                            self.metrics.counters.successful.increment(1);
                        },
                        Err(result) => {
                            error!("Error in sending push notification for {}: {:?} ({} ms)",
                                event.get_application_id(),
                                result, response_time / 1000000);

                            apns_result.set_status(Self::convert_status(result.status));
                            apns_result.set_successful(false);

                            if let Some(reason) = Self::convert_reason(result.reason) {
                                apns_result.set_reason(reason);
                            }

                            if let Some(ts) = result.timestamp {
                                apns_result.set_timestamp(ts.to_timespec().sec);
                            }

                            self.metrics.counters.failure.increment(1);
                        }
                    }

                    event.mut_apple().set_result(apns_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "apple", // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();

                    self.metrics.gauges.in_flight.decrement(1);
                },
                Ok((mut event, None)) => {
                    let mut apns_result        = ApnsResult::new();

                    apns_result.set_successful(false);
                    apns_result.set_status(Error);
                    apns_result.set_reason(MissingCertificate);

                    event.mut_apple().set_result(apns_result);

                    self.channel.basic_publish(
                        &*self.config.rabbitmq.response_exchange,
                        "apple", // routing key
                        false,   // mandatory
                        false,   // immediate
                        BasicProperties { ..Default::default() },
                        event.write_to_bytes().unwrap()).unwrap();

                    self.metrics.gauges.in_flight.decrement(1);
                    self.metrics.counters.certificate_missing.increment(1);
                },
                Err(_) => {
                    thread::park_timeout(wait_duration);
                },
            }
        }
    }

    fn is_running(&self) -> bool {
        self.control.load(Ordering::Relaxed) || self.metrics.gauges.in_flight.collect() > 0
    }

    fn convert_status(status: APNSStatus) -> ApnsResult_Status {
        match status {
            APNSStatus::Success        => Success,
            APNSStatus::BadRequest     => BadRequest,
            APNSStatus::MissingChannel => MissingChannel,
            APNSStatus::Timeout        => Timeout,
            APNSStatus::Unknown        => Unknown,
            _                          => Error,
        }
    }

    fn convert_reason(reason: Option<APNSError>) -> Option<ApnsResult_Reason> {
        match reason {
            Some(APNSError::PayloadEmpty)              => Some(PayloadEmpty),
            Some(APNSError::PayloadTooLarge)           => Some(PayloadTooLarge),
            Some(APNSError::BadTopic)                  => Some(BadTopic),
            Some(APNSError::TopicDisallowed)           => Some(TopicDisallowed),
            Some(APNSError::BadMessageId)              => Some(BadMessageId),
            Some(APNSError::BadExpirationDate)         => Some(BadExpirationDate),
            Some(APNSError::BadPriority)               => Some(BadPriority),
            Some(APNSError::MissingDeviceToken)        => Some(MissingDeviceToken),
            Some(APNSError::BadDeviceToken)            => Some(BadDeviceToken),
            Some(APNSError::DeviceTokenNotForTopic)    => Some(DeviceTokenNotForTopic),
            Some(APNSError::Unregistered)              => Some(Unregistered),
            Some(APNSError::DuplicateHeaders)          => Some(DuplicateHeaders),
            Some(APNSError::BadCertificateEnvironment) => Some(BadCertificateEnvironment),
            Some(APNSError::BadCertificate)            => Some(BadCertificate),
            Some(APNSError::Forbidden)                 => Some(Forbidden),
            Some(APNSError::BadPath)                   => Some(BadPath),
            Some(APNSError::MethodNotAllowed)          => Some(MethodNotAllowed),
            Some(APNSError::TooManyRequests)           => Some(TooManyRequests),
            Some(APNSError::IdleTimeout)               => Some(IdleTimeout),
            Some(APNSError::Shutdown)                  => Some(Shutdown),
            Some(APNSError::InternalServerError)       => Some(InternalServerError),
            Some(APNSError::ServiceUnavailable)        => Some(ServiceUnavailable),
            Some(APNSError::MissingTopic)              => Some(MissingTopic),
            _                                          => None,
        }
    }
}
