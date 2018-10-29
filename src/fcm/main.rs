#[macro_use] extern crate lazy_static;
#[macro_use] extern crate slog;
#[macro_use] extern crate slog_scope;

extern crate common;
extern crate fcm;
extern crate futures;

mod consumer;
mod notifier;
mod producer;

use common::{config::Config, system::System};

use consumer::FcmHandler;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config = match env::var("CONFIG") {
        Ok(config_file_location) => Config::parse(&config_file_location),
        _ => Config::parse("./config/fcm.toml"),
    };
}

fn main() {
    System::start(
        "fcm",
        FcmHandler::new(),
        &CONFIG,
    );
}
