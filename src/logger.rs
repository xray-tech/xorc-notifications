use std::sync::Arc;
use config::Config;
use log::{LogLevelFilter};
use gelf::{Logger, UdpBackend, Message, Error};
use std::env;
use env_logger;
use gelf::{Message as GelfMessage, Level as GelfLevel, Error as GelfError};
use events::push_notification::PushNotification;
use web_push::WebPushError;

pub struct GelfLogger {
    connection: Option<Logger>,
    filter: LogLevelFilter,
}

impl GelfLogger {
    pub fn new(config: Arc<Config>) -> Result<GelfLogger, Error> {
        let log_level_filter = match env::var("RUST_LOG") {
            Ok(val) => match val.as_ref() {
                "info"  => LogLevelFilter::Info,
                "debug" => LogLevelFilter::Debug,
                "warn"  => LogLevelFilter::Warn,
                "error" => LogLevelFilter::Error,
                _       => LogLevelFilter::Info
            },
            _ => LogLevelFilter::Info
        };

        if let Ok(_) = env::var("RUST_GELF") {
            let mut logger = Logger::new(Box::new(UdpBackend::new(&config.log.host)?))?;
            let mut env_logger = Logger::new(Box::new(UdpBackend::new(&config.log.host)?))?;

            logger.set_default_metadata(String::from("application_name"), String::from("web-push"));
            env_logger.set_default_metadata(String::from("application_name"), String::from("web-push"));

            if let Ok(environment) = env::var("RUST_ENV") {
                logger.set_default_metadata(String::from("environment"), String::from(environment.clone()));
                env_logger.set_default_metadata(String::from("environment"), String::from(environment.clone()));
            } else {
                logger.set_default_metadata(String::from("environment"), String::from("development"));
                env_logger.set_default_metadata(String::from("environment"), String::from("development"));
            };

            env_logger.install(log_level_filter)?;

            Ok(GelfLogger { connection: Some(logger), filter: log_level_filter })
        } else {
            env_logger::init().unwrap();
            Ok(GelfLogger { connection: None, filter: log_level_filter })
        }
    }

    pub fn log_message(&self, msg: Message) {
        match self.connection {
            Some(ref connection) => {
                connection.log_message(msg)
            },
            None => {
                let level: LogLevelFilter = msg.level().into();

                if self.filter <= level {
                    let metadata = msg.all_metadata().iter().fold(Vec::new(), |mut acc, (k, v)| {
                        acc.push(format!("{}: {}", k, v));
                        acc
                    }).join(", ");

                    println!("[{}] {}: ({})",
                             level,
                             msg.short_message(),
                             metadata);
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

        match error {
            Some(&WebPushError::BadRequest(Some(ref error_info))) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", String::from("BadRequest"))?;
                test_msg.set_metadata("errno", format!("{}", error_info.errno))?;
                test_msg.set_metadata("long_error", format!("{}", error_info.message))?;
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
