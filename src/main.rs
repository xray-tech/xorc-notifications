#[macro_use]
extern crate log;
extern crate env_logger;
extern crate gelf;
extern crate hyper;

#[macro_use]
extern crate prometheus;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate postgres_derive;

extern crate postgres;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate apns2;
extern crate amqp;
extern crate chan_signal;
extern crate argparse;
extern crate protobuf;
extern crate toml;
extern crate rustc_serialize;
extern crate time;

mod notifier;
mod events;
mod consumer;
mod logger;
mod metrics;
mod config;
mod certificate_registry;
mod producer;
mod consumer_supervisor;

use logger::GelfLogger;
use metrics::StatisticsServer;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use std::thread;
use std::thread::JoinHandle;
use chan_signal::{Signal, notify};
use argparse::{ArgumentParser, Store};
use config::Config;
use producer::{ResponseProducer, ApnsResponse};
use consumer_supervisor::ConsumerSupervisor;

fn main() {
    let mut config_file_location = String::from("./config/config.toml");
    let exit_signal = notify(&[Signal::INT, Signal::TERM]);

    let control = Arc::new(AtomicBool::new(true));

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Apple Push Notification System");
        ap.refer(&mut config_file_location)
            .add_option(&["-c", "--config"], Store,
                        "Config file (default: config.toml)");
        ap.parse_args_or_exit();
    }

    let config = Arc::new(Config::parse(config_file_location));
    let logger = Arc::new(GelfLogger::new(config.clone()).expect("Error initializing logger"));

    info!("Apple Push Notification System starting up!");

    let (tx_response, rx_response): (Sender<ApnsResponse>, Receiver<ApnsResponse>) = mpsc::channel();

    let mut threads : Vec<JoinHandle<_>> = Vec::new();

    threads.push({
        let mut supervisor = ConsumerSupervisor::new(config.clone(), control.clone(), tx_response, logger.clone());

        thread::spawn(move || {
            info!("Starting consumer supervisor thread...");
            supervisor.run();
            info!("Exiting consumer supervisor thread...");
        })
    });

    threads.push({
        let mut producer = ResponseProducer::new(config.clone(), rx_response, control.clone(), logger.clone());

        thread::spawn(move || {
            info!("Starting response producer thread...");
            producer.run();
            info!("Exiting response producer thread...");
        })
    });

    debug!("Starting statistics server...");
    let mut listening = StatisticsServer::handle();

    if let Some(_) = exit_signal.recv() {
        info!("Quitting the Apple Push Notification service");
        control.store(false, Ordering::Relaxed);

        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }

        listening.close().unwrap();
    }
}
