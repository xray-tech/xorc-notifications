#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate a2;
extern crate futures;
extern crate gelf;
extern crate heck;
extern crate serde_json;
extern crate tokio_timer;
extern crate common;

mod notifier;
mod consumer;
mod producer;

use consumer::ApnsHandler;
use std::env;

use common::{
    system::System,
    logger::GelfLogger,
    config::Config,
};

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
        "Apple push notification system",
        ApnsHandler::new(),
        &CONFIG
    );
}
