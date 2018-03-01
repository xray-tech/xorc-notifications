use std::sync::Arc;
use config::Config;
use log::LevelFilter;
use gelf::{Error, Logger, Message, UdpBackend, Level};
use std::env;
use env_logger;

#[derive(Debug)]
pub enum LogAction {
    ConsumerCreate,
    ConsumerRestart,
    ConsumerStart,
    ConsumerDelete,
    NotificationResult,
}

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
}
