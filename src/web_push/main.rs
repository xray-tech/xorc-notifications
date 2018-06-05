#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate gelf;
extern crate futures;
extern crate web_push;
extern crate tokio_signal;
extern crate common;
extern crate hyper;

mod consumer;
mod notifier;
mod producer;

use common::{
    logger::GelfLogger,
    config::Config,
    system::System,
};

use consumer::WebPushHandler;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config =
        match env::var("CONFIG") {
            Ok(config_file_location) => {
                Config::parse(&config_file_location)
            },
            _ => {
                Config::parse("./config/web_push.toml")
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
        "Web push notification system",
        WebPushHandler::new(),
        &CONFIG
    );
}
