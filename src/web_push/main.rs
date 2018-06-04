#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

extern crate gelf;
extern crate futures;
extern crate web_push;
extern crate chan_signal;
extern crate protobuf;
extern crate toml;
extern crate serde;
extern crate time;
extern crate tokio_signal;
extern crate base64;
extern crate common;
extern crate rdkafka;
extern crate chrono;
extern crate hyper;

mod consumer;
mod notifier;
mod producer;

use std::{
    thread::{
        self,
        JoinHandle,
    },
    env,
};

use common::{
    metrics::StatisticsServer,
    logger::GelfLogger,
    kafka::PushConsumer,
    config::Config,
};

use chan_signal::{Signal, notify};
use futures::sync::oneshot;
use consumer::WebPushHandler;

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
    let exit_signal = notify(&[Signal::INT, Signal::TERM]);

    info!("Web Push Notification service starting up!");

    let mut threads: Vec<JoinHandle<_>> = Vec::new();

    // Prometheus statistics server kill-switch
    let (server_tx, server_rx) = oneshot::channel();
    let (consumer_tx, consumer_rx) = oneshot::channel();

    threads.push({
        thread::spawn(move || {
            info!("Starting web-push consumer...");
            let handler = WebPushHandler::new();

            let mut consumer = PushConsumer::new(
                handler,
                &CONFIG.kafka,
                1
            );

            if let Err(error) = consumer.consume(consumer_rx) {
                error!("Error in consumer: {:?}", error);
            }

            info!("Exiting web-push consumer...");
        })
    });

    threads.push({
        thread::spawn(move || {
            debug!("Starting statistics server...");
            StatisticsServer::handle(server_rx);
            debug!("Exiting statistics server...");
        })
    });

    if let Some(_) = exit_signal.recv() {
        info!("Quitting the Web Push Notification service");

        server_tx.send(()).unwrap();
        consumer_tx.send(()).unwrap();

        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
