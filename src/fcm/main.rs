#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate common;
extern crate fcm;
extern crate futures;
extern crate gelf;

mod consumer;
mod notifier;
mod producer;

use common::{config::Config, logger::GelfLogger, system::System};

use consumer::FcmHandler;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config = match env::var("CONFIG") {
        Ok(config_file_location) => Config::parse(&config_file_location),
        _ => Config::parse("./config/fcm.toml"),
    };
    pub static ref GLOG: GelfLogger = GelfLogger::new(&CONFIG.log.host, "apns2").unwrap();
}

fn main() {
    System::start(
        "Google Firebase push notification system",
        FcmHandler::new(),
        &CONFIG,
    );
}
