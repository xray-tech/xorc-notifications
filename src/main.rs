#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate postgres_derive;
#[macro_use]
extern crate prometheus;
#[macro_use]
extern crate chan;

extern crate apns2;
extern crate argparse;
extern crate chan_signal;
extern crate env_logger;
extern crate futures;
extern crate gelf;
extern crate heck;
extern crate hyper;
extern crate lapin_futures as lapin;
extern crate postgres;
extern crate protobuf;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate rustc_serialize;
extern crate serde_json;
extern crate time;
extern crate tokio_core;
extern crate tokio_signal;
extern crate tokio_timer;
extern crate toml;

mod notifier;
mod events;
mod consumer;
mod logger;
mod metrics;
mod config;
mod certificate_registry;
mod producer;
mod consumer_supervisor;
mod error;

use logger::GelfLogger;
use metrics::StatisticsServer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use chan_signal::{notify, Signal};
use argparse::{ArgumentParser, Store};
use config::Config;
use consumer_supervisor::ConsumerSupervisor;
use futures::sync::oneshot;
use futures::sync::mpsc;
use producer::ResponseProducer;

fn main() {
    let mut config_file_location = String::from("./config/config.toml");
    let exit_signal = notify(&[Signal::INT, Signal::TERM]);
    let (sdone, rdone) = chan::sync(0);
    let (server_tx, server_rx) = oneshot::channel();

    let control = Arc::new(AtomicBool::new(true));

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

    let (producer_tx, producer_rx) = mpsc::channel(10000);

    threads.push({
        let mut supervisor =
            ConsumerSupervisor::new(config.clone(), control.clone(), producer_tx, logger.clone());

        thread::spawn(move || {
            info!("Starting consumer supervisor thread...");
            supervisor.run();
            info!("Exiting consumer supervisor thread...");
        })
    });

    threads.push({
        let mut producer = ResponseProducer::new(config.clone(), logger.clone());

        thread::spawn(move || {
            info!("Starting response producer thread...");
            producer.run(producer_rx, sdone);
            info!("Exiting response producer thread...");
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
            control.store(false, Ordering::Relaxed);

            for thread in threads {
                thread.thread().unpark();
                thread.join().unwrap();
            }
        },
        rdone.recv() => {
            info!("We killed ourselves, goodbye!");

            server_tx.send(()).unwrap();
            control.store(false, Ordering::Relaxed);

            for thread in threads {
                thread.thread().unpark();
                thread.join().unwrap();
            }
        }
    }
}
