#[macro_use] extern crate lazy_static;
#[macro_use] extern crate slog;
#[macro_use] extern crate slog_scope;

mod consumer;
mod notifier;
mod producer;

use crate::consumer::ApnsHandler;
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
        "apns2",
        ApnsHandler::new(),
        &CONFIG,
    );
}
