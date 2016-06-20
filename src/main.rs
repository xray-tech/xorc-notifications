#[macro_use]
extern crate log;
extern crate env_logger;
extern crate syslog;
extern crate hyper;

extern crate apns2;
extern crate amqp;
extern crate chan_signal;
extern crate argparse;
extern crate protobuf;
extern crate toml;
extern crate influent;
extern crate histogram;
extern crate rustc_serialize;
extern crate time;

mod notifier;
mod events;
mod consumer;
mod logger;
mod metrics;
mod config;
mod artifactory;
mod certificate_registry;
mod producer;

use log::*;
use syslog::Facility;
use logger::SyslogLogger;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use std::thread;
use std::thread::JoinHandle;
use chan_signal::{Signal, notify};
use argparse::{ArgumentParser, Store, StoreTrue};
use consumer::Consumer;
use metrics::sender::MetricsSender;
use metrics::Metrics;
use config::Config;
use certificate_registry::CertificateRegistry;
use producer::{ResponseProducer, ApnsResponse};

fn setup_logger() {
    match syslog::unix(Facility::LOG_USER) {
        Ok(writer) => {
            let _ = log::set_logger(|max_log_level| {
                max_log_level.set(SyslogLogger::get_log_level_filter());
                Box::new(SyslogLogger::new(writer))
            });
            println!("Initialized syslog logger");
        },
        Err(e) => {
            env_logger::init().unwrap();
            println!("No syslog, output to stderr, {}", e);
        },
    }
}

fn main() {
    setup_logger();

    info!("Apple Push Notification System starting up!");

    let exit_signal = notify(&[Signal::INT, Signal::TERM]);

    let mut number_of_threads = 1;
    let mut config_file_location = String::from("./config/config.toml");
    let mut sandbox = false;

    let control = Arc::new(AtomicBool::new(true));
    let metrics = Metrics::new();

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Apple Push Notification System");
        ap.refer(&mut number_of_threads)
            .add_option(&["-n", "--number_of_threads"], Store,
                        "Number of worker threads (default: 1)");
        ap.refer(&mut config_file_location)
            .add_option(&["-c", "--config"], Store,
                        "Config file (default: config.toml)");
        ap.refer(&mut sandbox)
            .add_option(&["-s", "--sandbox"], StoreTrue,
                        "Use the APNS sandbox environment");
        ap.parse_args_or_exit();
    }

    let config               = Arc::new(Config::parse(config_file_location));
    let certificate_registry = Arc::new(CertificateRegistry::new(config.clone()));
    let (tx_response, rx_response): (Sender<ApnsResponse>, Receiver<ApnsResponse>) = mpsc::channel();

    let mut threads : Vec<JoinHandle<_>> = (0..number_of_threads).map(|i| {
        let mut consumer = Consumer::new(control.clone(), metrics.clone(),
            config.clone(), certificate_registry.clone(), tx_response.clone());

        thread::spawn(move || {
            info!("Starting consumer #{}", i);

            if let Err(error) = consumer.consume(&sandbox) {
                error!("Error in consumer: {:?}", error);
            }

            info!("Exiting consumer #{}", i);
        })
    }).collect();

    threads.push({
        let metrics_sender = MetricsSender {
            metrics: metrics.clone(),
            control: control.clone(),
            config: config.clone(),
        };

        thread::spawn(move || {
            info!("Starting metrics thread...");
            metrics_sender.run();
            info!("Exiting metrics thread...");
        })
    });

    threads.push({
        let mut producer = ResponseProducer::new(config.clone(), rx_response,
            control.clone(), metrics.clone());

        thread::spawn(move || {
            info!("Starting response producer thread...");
            producer.run();
            info!("Exiting response producer thread...");
        })
    });


    if let Some(_) = exit_signal.recv() {
        info!("Quitting the Apple Push Notification service");
        control.store(false, Ordering::Relaxed);

        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
