#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate gelf;
extern crate futures;
extern crate http;
extern crate hyper;
extern crate fcm;
extern crate rdkafka;
extern crate chan_signal;
extern crate argparse;
extern crate protobuf;
extern crate toml;
extern crate serde;
extern crate chrono;
extern crate tokio_core;
extern crate tokio_signal;
extern crate common;

mod consumer;
mod notifier;
mod producer;

use common::{
    logger::GelfLogger,
    metrics::StatisticsServer,
    kafka::PushConsumer,
    config::Config,
};

use std::{
    thread::{self, JoinHandle},
    env,
};

use chan_signal::{Signal, notify};
use consumer::FcmHandler;
use futures::sync::oneshot;

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
    let exit_signal              = notify(&[Signal::INT, Signal::TERM]);

    info!("Google Firebase Push Notification service starting up!");

    let mut threads: Vec<JoinHandle<_>> = Vec::new();

    // Prometheus statistics server kill-switch
    let (server_tx, server_rx) = oneshot::channel();
    let (consumer_tx, consumer_rx) = oneshot::channel();

    {
        threads.push({
            thread::spawn(move || {
                info!("Starting fcm consumer...");

                let handler = FcmHandler::new();

                let mut consumer = PushConsumer::new(
                    handler,
                    &CONFIG.kafka,
                    1
                );

                if let Err(error) = consumer.consume(consumer_rx) {
                    error!("Error in consumer: {:?}", error);
                }

                info!("Exiting fcm consumer ...");
            })
        });

        threads.push({
            thread::spawn(move || {
                debug!("Starting statistics server...");
                StatisticsServer::handle(server_rx);
            })
        });
    }

    if let Some(_) = exit_signal.recv() {
        info!("Quitting the Google Firebase Push Notification service");

        server_tx.send(()).unwrap();
        consumer_tx.send(()).unwrap();

        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
