#[macro_use] extern crate lazy_static;
#[macro_use] extern crate slog;
#[macro_use] extern crate slog_scope;

extern crate tokio_timer;
extern crate protobuf;
extern crate common;
extern crate fcm;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate http;
extern crate bytes;
extern crate chrono;

mod consumer;
mod requester;
mod producer;

use common::{config::Config, system::System};

use consumer::HttpRequestHandler;
use std::env;

lazy_static! {
    pub static ref CONFIG: Config = match env::var("CONFIG") {
        Ok(config_file_location) => Config::parse(&config_file_location),
        _ => Config::parse("./config/http_requester.toml"),
    };
}

fn main() {
    System::start("http_requester", HttpRequestHandler::new(), &CONFIG);
}
