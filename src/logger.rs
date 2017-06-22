use std::sync::Arc;
use config::Config;
use log::{LogLevelFilter};
use gelf::{Logger, UdpBackend, Message, Error};
use std::env;
use env_logger;

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
}
