#[macro_use] extern crate lazy_static;
#[macro_use] extern crate slog;
#[macro_use] extern crate slog_scope;

extern crate a2;
extern crate common;
extern crate futures;
extern crate heck;
extern crate serde_json;
extern crate tokio_timer;

mod consumer;
mod notifier;
mod producer;

use consumer::ApnsHandler;
use std::env;

use common::{config::Config, system::System};

lazy_static! {
    pub static ref CONFIG: Config = match env::var("CONFIG") {
        Ok(config_file_location) => Config::parse(&config_file_location),
        _ => Config::parse("./config/fcm.toml"),
    };
}

fn main() {
    System::start(
        "Apple push notification system",
        ApnsHandler::new(),
        &CONFIG,
    );
}
