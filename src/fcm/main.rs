#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate gelf;
extern crate common;
extern crate futures;
extern crate fcm;

mod consumer;
mod notifier;
mod producer;

use common::{
    system::System,
    logger::GelfLogger,
    config::Config,
};

use std::env;
use consumer::FcmHandler;

lazy_static! {
    pub static ref CONFIG: Config =
        match env::var("CONFIG") {
            Ok(config_file_location) => {
                Config::parse(&config_file_location)
            },
            _ => {
                Config::parse("./config/fcm.toml")
            }
        };

    pub static ref GLOG: GelfLogger =
        GelfLogger::new(
            &CONFIG.log.host,
            "apns2"
        ).unwrap();
}

fn main() {
    System::start(
        "Google Firebase push notification system",
        FcmHandler::new(),
        &CONFIG,
    );
}
