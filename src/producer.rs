use std::sync::Arc;
use tokio_core::reactor::Core;
use tokio_core::net::TcpStream;
use lapin::client::{Client, ConnectionOptions};
use lapin::channel::*;
use lapin::types::FieldTable;
use apns2::error::Error;
use apns2::response::{ErrorReason, Response};
use events::header::Header;
use events::push_notification::PushNotification;
use events::apple_notification::*;
use events::notification_result::{NotificationResult, NotificationResult_Error};
use events::apple_notification::ApnsResult_Reason::*;
use events::apple_notification::ApnsResult_Status::*;
use config::Config;
use heck::SnakeCase;
use logger::{GelfLogger, LogAction};
use gelf::{Error as GelfError, Level as GelfLevel, Message as GelfMessage};
use futures::{Future, Stream};
use futures::future::ok;
use futures::sync::mpsc::Receiver;
use std::thread;
use std::net::{SocketAddr, ToSocketAddrs};
use protobuf::core::Message;
use std::io;
use std::str::FromStr;
use time;
use metrics::*;
use consumer_supervisor::{CONSUMER_FAILURES, MAX_FAILURES};
use chan;

pub type ApnsData = (PushNotification, Result<Response, Error>);

pub struct ResponseProducer {
    config: Arc<Config>,
    logger: Arc<GelfLogger>,
}

#[derive(Debug, PartialEq)]
enum Retry {
    Yes,
    No,
}

impl ResponseProducer {
    pub fn new(config: Arc<Config>, logger: Arc<GelfLogger>) -> ResponseProducer {
        ResponseProducer {
            config: config,
            logger: logger,
        }
    }

    pub fn run(&mut self, rx_consumers: Receiver<ApnsData>, panic_button: chan::Sender<()>) {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let address: SocketAddr = format!(
            "{}:{}",
            self.config.rabbitmq.host, self.config.rabbitmq.port
        ).to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();

        let connection_options = ConnectionOptions {
            username: self.config.rabbitmq.login.clone(),
            password: self.config.rabbitmq.password.clone(),
            vhost: self.config.rabbitmq.vhost.clone(),
            ..Default::default()
        };

        let work = TcpStream::connect(&address, &handle)
            .and_then(move |stream| Client::connect(stream, &connection_options))
            .and_then(|(client, heartbeat_future_fn)| {
                let heartbeat_client = client.clone();

                thread::Builder::new()
                    .name("heartbeat thread".to_string())
                    .spawn(move || {
                        match Core::new()
                            .unwrap()
                            .run(heartbeat_future_fn(&heartbeat_client))
                        {
                            Ok(s) => {
                                info!("Producer heartbeat thread exited cleanly ({:?})", s);
                            },
                            Err(e) => {
                                error!("Producer heartbeat thread crashed, going down... ({:?})", e);
                                panic_button.send(());
                            },
                        }
                    })
                    .unwrap();

                client.create_channel()
            })
            .and_then(|channel| {
                channel.exchange_declare(
                    &self.config.rabbitmq.response_exchange,
                    &self.config.rabbitmq.response_exchange_type,
                    &ExchangeDeclareOptions {
                        passive: false,
                        durable: true,
                        auto_delete: false,
                        internal: false,
                        nowait: false,
                        ..Default::default()
                    },
                    &FieldTable::new(),
                ).and_then(move |_| {
                    rx_consumers.for_each(move |item| {
                        let work = match item {
                            (event, Ok(response)) => {
                                let _ = Self::log_result(
                                    "Successfully sent a push notification",
                                    &self.logger, &event, Some(&response), None);

                                Self::handle_response(
                                    &channel, &self.config.rabbitmq.response_exchange,
                                    event)
                            }
                            (event, Err(Error::ResponseError(response))) => {
                                let _ = Self::log_result(
                                    "Error sending a push notification",
                                    &self.logger, &event, Some(&response), None);

                                Self::handle_error(
                                    &channel, &self.config.rabbitmq.response_exchange,
                                    event, response)
                            }
                            (event, Err(Error::TimeoutError)) => {
                                let _ = Self::log_result(
                                    "Timeout when sending a push notification",
                                    &self.logger, &event, None, Some(Error::TimeoutError));

                                Self::mark_failure(event.get_application_id(), 1);

                                Self::handle_fatal(
                                    &channel, &self.config.rabbitmq.response_exchange,
                                    ApnsResult_Status::Timeout, event)
                            }
                            (event, Err(Error::ConnectionError)) => {
                                let _ = Self::log_result(
                                    "Connection error",
                                    &self.logger, &event, None, Some(Error::ConnectionError));

                                Self::mark_failure(event.get_application_id(), MAX_FAILURES);

                                Self::handle_fatal(
                                    &channel, &self.config.rabbitmq.response_exchange,
                                    ApnsResult_Status::MissingChannel, event)
                            }
                            (event, Err(e)) => {
                                error!("Fatal error when sending a push notification, restarting consumer: {:?}", e);

                                Self::mark_failure(event.get_application_id(), MAX_FAILURES);

                                Self::handle_fatal(
                                    &channel, &self.config.rabbitmq.response_exchange,
                                    ApnsResult_Status::Unknown, event)
                            }
                        }.then(|_| ok(()));

                        handle.spawn(work);

                        Ok(())
                    }).then(|_| ok(()))
                })
            });

        core.run(work).unwrap();
    }

    fn can_reply(event: &PushNotification, routing_key: Retry) -> bool {
        event.has_exchange() && routing_key == Retry::No && event.has_response_recipient_id()
    }

    fn publish(
        channel: &Channel<TcpStream>,
        event: PushNotification,
        exchange: &str,
        routing_key: Retry,
    ) -> Box<Future<Item = Option<bool>, Error = io::Error>> {
        if Self::can_reply(&event, routing_key) {
            let response_routing_key = event.get_response_recipient_id();
            let response = event.get_apple().get_result();
            let current_time = time::get_time();
            let mut header = Header::new();

            header.set_created_at(
                (current_time.sec as i64 * 1000) + (current_time.nsec as i64 / 1000 / 1000),
            );
            header.set_source(String::from("apns"));
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
                }
                Unregistered => {
                    result_event.set_delete_user(true);
                    result_event.set_successful(false);
                    result_event.set_error(NotificationResult_Error::Unsubscribed);
                }
                _ => {
                    match response.get_reason() {
                        DeviceTokenNotForTopic | BadDeviceToken => {
                            result_event.set_delete_user(true);
                            result_event.set_successful(false);
                            result_event.set_error(NotificationResult_Error::Unsubscribed);
                        },
                        _ => {
                            result_event.set_delete_user(false);
                            result_event.set_successful(false);
                            result_event.set_reason(format!("{:?}", response.get_status()));
                            result_event.set_error(NotificationResult_Error::Other);
                        }
                    }
                }
            }

            channel.basic_publish(
                event.get_exchange(),
                response_routing_key,
                &result_event.write_to_bytes().unwrap(),
                &BasicPublishOptions {
                    mandatory: false,
                    immediate: false,
                    ..Default::default()
                },
                BasicProperties::default(),
            )
        } else {
            channel.basic_publish(
                exchange,
                "no_retry",
                &event.write_to_bytes().unwrap(),
                &BasicPublishOptions {
                    mandatory: false,
                    immediate: false,
                    ..Default::default()
                },
                BasicProperties::default(),
            )
        }
    }

    fn handle_response(
        channel: &Channel<TcpStream>,
        exchange: &str,
        mut event: PushNotification,
    ) -> Box<Future<Item = Option<bool>, Error = io::Error>> {
        let mut apns_result = ApnsResult::new();

        apns_result.set_successful(true);
        apns_result.set_status(ApnsResult_Status::Success);

        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        event.mut_apple().set_result(apns_result);

        Self::publish(channel, event, exchange, Retry::No)
    }

    fn handle_fatal(
        channel: &Channel<TcpStream>,
        exchange: &str,
        status: ApnsResult_Status,
        mut event: PushNotification,
    ) -> Box<Future<Item = Option<bool>, Error = io::Error>> {
        let mut apns_result = ApnsResult::new();
        let status_label = format!("{:?}", status).to_snake_case();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        let retry_after = if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        };

        CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();

        event.mut_apple().set_result(apns_result);
        event.set_retry_after(retry_after);

        Self::publish(channel, event, exchange, Retry::Yes)
    }

    fn handle_error(
        channel: &Channel<TcpStream>,
        exchange: &str,
        mut event: PushNotification,
        response: Response,
    ) -> Box<Future<Item = Option<bool>, Error = io::Error>> {
        let mut apns_result = ApnsResult::new();
        let status: ApnsResult_Status = response.code.into();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        if let Some(ref error) = response.error {
            match error.reason {
                ErrorReason::ExpiredProviderToken
                    | ErrorReason::IdleTimeout
                    | ErrorReason::BadCertificate => Self::mark_failure(event.get_application_id(), MAX_FAILURES),
                _ => (),
            }

            apns_result.set_reason((&error.reason).into());

            if let Some(ts) = error.timestamp {
                apns_result.set_timestamp(ts as i64);
            }
        };

        let retry_after = if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        };

        match response.error {
            Some(ref reason) => {
                let reason_label = format!("{:?}", reason.reason).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&reason_label]).inc();
            }
            None => {
                let status_label = format!("{:?}", status).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
            }
        }

        let routing_key = match apns_result.get_reason() {
            InternalServerError | Shutdown | ServiceUnavailable | ExpiredProviderToken => {
                event.set_retry_after(retry_after);
                Retry::Yes
            }
            _ => match apns_result.get_status() {
                Timeout | Unknown | Forbidden => {
                    event.set_retry_after(retry_after);
                    Retry::Yes
                }
                _ => Retry::No,
            },
        };

        event.mut_apple().set_result(apns_result);
        Self::publish(channel, event, exchange, routing_key)
    }

    fn mark_failure(app_id: &str, error_level: i32) {
        match i32::from_str(app_id) {
            Ok(id) => {
                let mut failures = CONSUMER_FAILURES.lock().unwrap();
                let counter = failures.entry(id).or_insert(0);
                *counter += error_level;
            }
            _ => (),
        }
    }

    fn log_result<'a>(
        title: &str,
        logger: &Arc<GelfLogger>,
        event: &PushNotification,
        response: Option<&Response>,
        error: Option<Error>
    ) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));
        test_msg.set_metadata("action", format!("{:?}", LogAction::NotificationResult))?;

        test_msg
            .set_full_message(format!("{:?}", event))
            .set_level(GelfLevel::Informational)
            .set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?
            .set_metadata("device_token", format!("{}", event.get_device_token()))?
            .set_metadata("app_id", format!("{}", event.get_application_id()))?
            .set_metadata("campaign_id", format!("{}", event.get_campaign_id()))?
            .set_metadata(
                "event_source",
                String::from(event.get_header().get_source()),
            )?;

        if let Some(r) = response {
            match r.code {
                200 => {
                    test_msg.set_metadata("successful", String::from("true"))?;
                }
                code => {
                    let error: ApnsResult_Status = code.into();
                    test_msg.set_metadata("successful", String::from("false"))?;
                    test_msg.set_metadata("error", format!("{:?}", error))?;

                    if let Some(ref reason) = r.error {
                        test_msg.set_metadata("reason", format!("{:?}", reason.reason))?;
                    }
                }
            }
        } else {
            test_msg.set_metadata("successful", String::from("false"))?;

            if let Some(e) = error {
                test_msg.set_metadata("error", format!("{:?}", e))?;
            }
        }

        logger.log_message(test_msg);

        Ok(())
    }
}
