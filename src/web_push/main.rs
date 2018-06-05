#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate common;
extern crate futures;
extern crate gelf;
extern crate hyper;
extern crate tokio_signal;
extern crate web_push;

mod consumer;
mod notifier;
mod producer;

use common::{config::Config, logger::GelfLogger, system::System};

use consumer::WebPushHandler;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config = match env::var("CONFIG") {
        Ok(config_file_location) => Config::parse(&config_file_location),
        _ => Config::parse("./config/web_push.toml"),
    };
    pub static ref GLOG: GelfLogger = GelfLogger::new(&CONFIG.log.host, "apns2").unwrap();
}

fn main() {
    System::start(
        "Web push notification system",
        WebPushHandler::new(),
        &CONFIG,
    );
}
