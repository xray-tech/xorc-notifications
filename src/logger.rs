use std::sync::Arc;
use config::Config;
use log::LevelFilter;
use gelf::{Error, Logger, Message, UdpBackend, Level};
use std::env;
use env_logger;
use gelf::{Message as GelfMessage, Level as GelfLevel, Error as GelfError};
use hyper::Uri;
use events::push_notification::PushNotification;
use web_push::WebPushError;


pub struct GelfLogger {
    connection: Option<Logger>,
    filter: LevelFilter,
}

impl GelfLogger {
    pub fn new(config: Arc<Config>) -> Result<GelfLogger, Error> {
        let log_level_filter = match env::var("RUST_LOG") {
            Ok(val) => match val.as_ref() {
                "info" => LevelFilter::Info,
                "debug" => LevelFilter::Debug,
                "warn" => LevelFilter::Warn,
                "error" => LevelFilter::Error,
                _ => LevelFilter::Info,
            },
            _ => LevelFilter::Info,
        };

        if let Ok(_) = env::var("RUST_GELF") {
            let mut logger = Logger::new(Box::new(UdpBackend::new(&config.log.host)?))?;
            let mut env_logger = Logger::new(Box::new(UdpBackend::new(&config.log.host)?))?;

            logger.set_default_metadata(String::from("application_name"), String::from("apns2"));
            env_logger.set_default_metadata(String::from("application_name"), String::from("apns2"));

            if let Ok(environment) = env::var("RUST_ENV") {
                logger.set_default_metadata(
                    String::from("environment"),
                    String::from(environment.clone()),
                );
                env_logger.set_default_metadata(
                    String::from("environment"),
                    String::from(environment.clone()),
                );
            } else {
                logger
                    .set_default_metadata(String::from("environment"), String::from("development"));
                env_logger
                    .set_default_metadata(String::from("environment"), String::from("development"));
            };

            let filter = match env::var("RUST_LOG") {
                Ok(val) => match val.as_ref() {
                    "info" => Level::Informational,
                    "debug" => Level::Debug,
                    "warn" => Level::Warning,
                    "error" => Level::Error,
                    _ => Level::Informational,
                },
                _ => Level::Informational,
            };

            env_logger.install(filter)?;

            Ok(GelfLogger {
                connection: Some(logger),
                filter: log_level_filter,
            })
        } else {
            env_logger::init();

            Ok(GelfLogger {
                connection: None,
                filter: log_level_filter,
            })
        }
    }

    pub fn log_message(&self, msg: Message) {
        match self.connection {
            Some(ref connection) => connection.log_message(msg),
            None => {
                let level = match msg.level() {
                    Level::Emergency | Level::Alert | Level::Critical | Level::Error => LevelFilter::Error,
                    Level::Warning => LevelFilter::Warn,
                    Level::Notice | Level::Informational => LevelFilter::Info,
                    Level::Debug => LevelFilter::Debug,
                };

                if self.filter <= level {
                    let metadata = msg.all_metadata()
                        .iter()
                        .fold(Vec::new(), |mut acc, (k, v)| {
                            acc.push(format!("{}: {}", k, v));
                            acc
                        })
                        .join(", ");

                    println!("[{}] {}: ({})", level, msg.short_message(), metadata);
                }
            }
        }
    }

    pub fn log_push_result(&self, title: &str, event: &PushNotification, error: Option<&WebPushError>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

        if let Ok(uri) = event.get_device_token().parse::<Uri>() {
            if let Some(host) = uri.host() {
                test_msg.set_metadata("push_service", String::from(host))?;
            };
        };

        match error {
            Some(&WebPushError::BadRequest(Some(ref error_info))) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", String::from("BadRequest"))?;
                test_msg.set_metadata("long_error", format!("{}", error_info))?;
            }
            Some(error_msg) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", format!("{:?}", error_msg))?;
            },
            _ => {
                test_msg.set_metadata("successful", String::from("true"))?;
            }
        }

        self.log_message(test_msg);

        Ok(())
    }
}
