#[macro_use]
extern crate log;
extern crate env_logger;
extern crate syslog;
extern crate hyper;

extern crate fcm;
extern crate amqp;
extern crate chan_signal;
extern crate argparse;
extern crate protobuf;
extern crate toml;
extern crate influent;
extern crate histogram;
extern crate rustc_serialize;
extern crate time;
extern crate postgres;
extern crate r2d2;
extern crate r2d2_postgres;

mod logger;
mod config;
mod consumer;
mod notifier;
mod events;
mod producer;
mod metrics;
mod certificate_registry;

use log::*;
use config::Config;
use syslog::Facility;
use logger::SyslogLogger;
use chan_signal::{Signal, notify};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use argparse::{ArgumentParser, Store};
use std::thread::{self, JoinHandle};
use consumer::Consumer;
use notifier::Notifier;
use producer::{FcmData, ResponseProducer};
use std::sync::mpsc;
use std::sync::mpsc::{Sender, Receiver};
use metrics::sender::MetricsSender;
use metrics::Metrics;
use certificate_registry::CertificateRegistry;

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

    info!("Google Firebase Push Notification service starting up!");

    let exit_signal              = notify(&[Signal::INT, Signal::TERM]);
    let mut number_of_threads    = 1;
    let mut config_file_location = String::from("./config/config.toml");

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Google Firebase Push Notification System");
        ap.refer(&mut number_of_threads)
            .add_option(&["-n", "--number_of_threads"], Store,
                        "Number of worker threads (default: 1)");
        ap.refer(&mut config_file_location)
            .add_option(&["-c", "--config"], Store,
                        "Config file (default: ./config/config.toml)");
        ap.parse_args_or_exit();
    }

    let metrics              = Metrics::new();
    let config               = Arc::new(Config::parse(config_file_location));
    let certificate_registry = Arc::new(CertificateRegistry::new(config.clone()));
    let control              = Arc::new(AtomicBool::new(true));

    let (tx, rx): (Sender<FcmData>, Receiver<FcmData>) = mpsc::channel();

    let mut threads : Vec<JoinHandle<_>> = (0..number_of_threads).map(|i| {
        let notifier     = Notifier::new(metrics.clone());
        let mut consumer = Consumer::new(control.clone(), config.clone(), notifier,
            tx.clone(), certificate_registry.clone());

        thread::spawn(move || {
            info!("Starting consumer #{}", i);

            if let Err(error) = consumer.consume() {
                error!("Error in consumer: {:?}", error);
            }

            info!("Exiting consumer #{}", i);
        })
    }).collect();

    threads.push({
        let mut producer = ResponseProducer::new(config.clone(), rx, control.clone(), metrics.clone());

        thread::spawn(move || {
            info!("Starting response producer thread...");
            producer.run();
            info!("Exiting response producer thread...");
        })
    });

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

    if let Some(_) = exit_signal.recv() {
        info!("Quitting the Google Firebase Push Notification service");

        control.store(false, Ordering::Relaxed);

        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
