#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate chan;
#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate a2;
extern crate argparse;
extern crate chan_signal;
extern crate env_logger;
extern crate futures;
extern crate gelf;
extern crate heck;
extern crate hyper;
extern crate rdkafka;
extern crate protobuf;
extern crate serde_json;
extern crate time;
extern crate tokio_core;
extern crate tokio_signal;
extern crate tokio_timer;
extern crate toml;

mod notifier;
mod events;
mod consumer;
mod producer;
mod logger;
mod metrics;
mod config;
mod error;

use logger::GelfLogger;
use metrics::StatisticsServer;
use std::{
    sync::Arc,
    thread,
    thread::JoinHandle,
};
use chan_signal::{notify, Signal};
use argparse::{ArgumentParser, Store};
use config::Config;
use futures::{
    sync::oneshot,
};
use consumer::ApnsConsumer;

fn main() {
    let mut config_file_location = String::from("./config/config.toml");
    let exit_signal = notify(&[Signal::INT, Signal::TERM]);
    let (server_tx, server_rx) = oneshot::channel();
    let (consumer_tx, consumer_rx) = oneshot::channel();

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Apple Push Notification System");
        ap.refer(&mut config_file_location).add_option(
            &["-c", "--config"],
            Store,
            "Config file (default: config.toml)",
        );
        ap.parse_args_or_exit();
    }

    let config = Arc::new(Config::parse(config_file_location));
    let logger = Arc::new(GelfLogger::new(config.clone()).expect("Error initializing logger"));

    info!("Apple Push Notification System starting up!");

    let mut threads: Vec<JoinHandle<_>> = Vec::new();

    threads.push({
        thread::spawn(move || {
            debug!("Starting apns consumer...");
            let mut consumer = ApnsConsumer::new(config.clone(), logger.clone(), 1);
            consumer.consume(consumer_rx).unwrap();
        })
    });

    threads.push({
        thread::spawn(move || {
            debug!("Starting statistics server...");
            StatisticsServer::handle(server_rx);
        })
    });

    chan_select! {
        exit_signal.recv() -> signal => {
            info!("Received signal: {:?}", signal);

            server_tx.send(()).unwrap();
            consumer_tx.send(()).unwrap();

            for thread in threads {
                thread.thread().unpark();
                thread.join().unwrap();
            }
        },
    }
}
