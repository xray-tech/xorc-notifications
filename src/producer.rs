use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::{i32, thread};
use events::notification_result::{NotificationResult, NotificationResult_Error};
use events::push_notification::PushNotification;
use events::apple_notification::ApnsResult;
use events::apple_notification::ApnsResult_Status::*;
use events::apple_notification::ApnsResult_Reason::*;
use events::header::Header;
use time::{self, precise_time_ns};
use config::Config;
use amqp::{Session, Channel, Table, Basic, Options};
use amqp::protocol::basic::BasicProperties;
use apns2::client::{ProviderResponse, Response, APNSStatus};
use protobuf::core::Message;
use metrics::{CALLBACKS_COUNTER, CALLBACKS_INFLIGHT, RESPONSE_TIMES_HISTOGRAM};
use logger::{GelfLogger, LogAction};
use gelf::{Message as GelfMessage, Level as GelfLevel, Error as GelfError};
use consumer_supervisor::CONSUMER_FAILURES;
use std::str::FromStr;
use heck::SnakeCase;

pub type ApnsResponse = (PushNotification, Option<ProviderResponse>);

pub struct ResponseProducer {
    config: Arc<Config>,
    session: Session,
    channel: Channel,
    rx: Receiver<(PushNotification, Option<ProviderResponse>)>,
    control: Arc<AtomicBool>,
    logger: Arc<GelfLogger>,
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

    fn can_reply(event: &PushNotification, routing_key: &str) -> bool {
        event.has_exchange() && routing_key == "no_retry" && event.has_response_recipient_id()
    }

    fn publish(&mut self, event: PushNotification, routing_key: &str) {
        if Self::can_reply(&event, routing_key) {
            let response_routing_key = event.get_response_recipient_id();
            let response             = event.get_apple().get_result();
            let current_time         = time::get_time();
            let mut header           = Header::new();

            header.set_created_at((current_time.sec as i64 * 1000) +
                                  (current_time.nsec as i64 / 1000 / 1000));
            header.set_source(String::from("apns"));
            header.set_recipient_id(String::from(response_routing_key));
            header.set_field_type(String::from("notification.NotificationResult"));

            let mut result_event = NotificationResult::new();
            result_event.set_header(header);
            result_event.set_universe(String::from(event.get_universe()));
            result_event.set_correlation_id(String::from(event.get_correlation_id()));

            match response.get_status() {
                Success => {
                    result_event.set_successful(true);
                },
                Unregistered => {
                    result_event.set_successful(false);
                    result_event.set_error(NotificationResult_Error::Unsubscribed);
                },
                _ => {
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
                event.write_to_bytes().expect("Couldn't serialize a protobuf event")).expect("Couldn't publish to RabbitMQ");
        }

        self.channel.basic_publish(
            &*self.config.rabbitmq.response_exchange,
            routing_key,
            false,   // mandatory
            false,   // immediate
            BasicProperties { ..Default::default() },
            event.write_to_bytes().expect("Couldn't serialize a protobuf event")).expect("Couldn't publish to RabbitMQ");
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
                            apns_result.set_status((&result.status).into());

                            match result.reason {
                                Some(reason) => {
                                    let reason_label = format!("{:?}", reason).to_snake_case();
                                    CALLBACKS_COUNTER.with_label_values(&[&reason_label]).inc();
                                },
                                None => {
                                    let status_label = format!("{:?}", result.status).to_snake_case();
                                    CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
                                },
                            }

                            "no_retry"
                        },
                        Err(result) => {
                            apns_result.set_status((&result.status).into());
                            apns_result.set_successful(false);

                            if let Some(ref reason) = result.reason {
                                apns_result.set_reason(reason.into())
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

                            match result.reason {
                                Some(ref reason) => {
                                    let reason_label = format!("{:?}", reason).to_snake_case();
                                    CALLBACKS_COUNTER.with_label_values(&[&reason_label]).inc();
                                },
                                None => {
                                    let status_label = format!("{:?}", result.status).to_snake_case();
                                    CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
                                },
                            }

                            match apns_result.get_reason() {
                                InternalServerError | Shutdown | ServiceUnavailable | ExpiredProviderToken => {
                                    let _ = self.log_result("Retrying a push notification", &event, Some(&result));

                                    event.set_retry_after(retry_after);
                                    "retry"
                                },
                                _ => match apns_result.get_status() {
                                    MissingChannel => {
                                        let _ = self.log_result("Retrying a push notification", &event, Some(&result));

                                        match i32::from_str(event.get_application_id()) {
                                            Ok(app_id) => {
                                                let mut failures = CONSUMER_FAILURES.lock().unwrap();

                                                failures.insert(app_id);
                                            },
                                            Err(_) => error!("Faulty app-id: {}", event.get_application_id())
                                        }

                                        event.set_retry_after(retry_after);
                                        "retry"
                                    },
                                    Timeout | Unknown | Forbidden => {
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

                    self.publish(event, routing_key);

                    CALLBACKS_INFLIGHT.dec();
                },
                Ok((mut event, None)) => {
                    let _ = self.log_result("Error sending a push notification", &event, None);

                    let mut apns_result = ApnsResult::new();

                    apns_result.set_successful(false);
                    apns_result.set_status(Error);
                    apns_result.set_reason(MissingCertificate);

                    event.mut_apple().set_result(apns_result);

                    self.publish(event, "no_retry");

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

    fn log_result(&self, title: &str, event: &PushNotification, response: Option<&Response>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));
        test_msg.set_metadata("action", format!("{:?}", LogAction::NotificationResult))?;

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
