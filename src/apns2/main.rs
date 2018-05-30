#[macro_use]
extern crate chan;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate serde;
extern crate a2;
extern crate argparse;
extern crate chan_signal;
extern crate futures;
extern crate gelf;
extern crate heck;
extern crate rdkafka;
extern crate protobuf;
extern crate serde_json;
extern crate time;
extern crate tokio_core;
extern crate tokio_signal;
extern crate tokio_timer;
extern crate toml;
extern crate common;
extern crate chrono;

mod notifier;
mod consumer;
mod producer;
mod config;

use common::{
    metrics::StatisticsServer,
    logger::GelfLogger,
};

use std::{
    thread,
    thread::JoinHandle,
    env,
};

use futures::{
    sync::oneshot,
};

use consumer::ApnsConsumer;
use chan_signal::{notify, Signal};
use config::Config;

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
    let exit_signal = notify(&[Signal::INT, Signal::TERM]);
    let (server_tx, server_rx) = oneshot::channel();
    let (consumer_tx, consumer_rx) = oneshot::channel();

    info!("Apple Push Notification System starting up!");

    let mut threads: Vec<JoinHandle<_>> = Vec::new();

    threads.push({
        thread::spawn(move || {
            debug!("Starting apns consumer...");
            let mut consumer = ApnsConsumer::new(1);
            consumer.consume(consumer_rx).unwrap();
            debug!("Exiting apns consumer...");
        })
    });

    threads.push({
        thread::spawn(move || {
            debug!("Starting statistics server...");
            StatisticsServer::handle(server_rx);
            debug!("Exiting statistics server...");
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
