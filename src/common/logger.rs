use log::LevelFilter;
use gelf::{Error, Logger, Message, UdpBackend, Level};
use std::env;
use env_logger;
use events::application::Application;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub host: String,
}

#[derive(Debug)]
pub enum LogAction {
    ConsumerCreate,
    ConsumerDelete,
    NotificationResult,
}

pub struct GelfLogger {
    connection: Option<Logger>,
    filter: LevelFilter,
}

impl GelfLogger {
    pub fn new(host: &str, application_name: &str) -> Result<GelfLogger, Error> {
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
            let mut logger = Logger::new(Box::new(UdpBackend::new(host)?))?;
            let mut env_logger = Logger::new(Box::new(UdpBackend::new(host)?))?;

            logger.set_default_metadata(
                "application_name",
                application_name
            );

            env_logger.set_default_metadata(
                "application_name",
                application_name
            );

            if let Ok(environment) = env::var("RUST_ENV") {
                logger.set_default_metadata(
                    "environment",
                    environment.clone(),
                );
                env_logger.set_default_metadata(
                    "environment",
                    environment.clone(),
                );
            } else {
                logger.set_default_metadata(
                    "environment",
                    "development"
                );
                env_logger.set_default_metadata(
                    "environment",
                    "development"
                );
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

    pub fn log_config_change(
        &self,
        title: &str,
        application: &Application,
    ) -> Result<(), Error>
    {
        let mut test_msg = Message::new(title);

        test_msg.set_metadata("app_id", application.get_id())?;

        if application.has_ios() {
            let ios_app = application.get_ios();

            if ios_app.has_token() {
                test_msg.set_metadata("endpoint", format!("{:?}", ios_app.get_endpoint()))?;
                test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
                test_msg.set_metadata("connection_type", "token")?;

                let token = ios_app.get_token();

                test_msg.set_metadata("key_id", token.get_key_id())?;
                test_msg.set_metadata("team_id", token.get_team_id())?;
            } else if ios_app.has_certificate() {
                test_msg.set_metadata("endpoint", format!("{:?}", ios_app.get_endpoint()))?;
                test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
                test_msg.set_metadata("connection_type", "certificate")?;
            }
        } else if application.has_android() {
            let android_app = application.get_android();
            test_msg.set_metadata("api_key", android_app.get_fcm_api_key())?;
        } else if application.has_web() {
            let web_app = application.get_web();
            test_msg.set_metadata("fcm_api_key", web_app.get_fcm_api_key())?;
        }

        self.log_message(test_msg);

        Ok(())
    }
}
