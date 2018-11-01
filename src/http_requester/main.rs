#[macro_use] extern crate lazy_static;
#[macro_use] extern crate slog;
#[macro_use] extern crate slog_scope;

mod consumer;
mod requester;
mod producer;

use common::{config::Config, system::System};
use crate::consumer::HttpRequestHandler;
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
