#[macro_use]
extern crate log;
extern crate env_logger;
extern crate syslog;
extern crate hyper;
extern crate futures;
extern crate tokio_core;

extern crate fcm;
extern crate amqp;
extern crate chan_signal;
extern crate argparse;
extern crate protobuf;
extern crate toml;
extern crate rustc_serialize;
extern crate time;
extern crate postgres;
extern crate r2d2;
extern crate r2d2_postgres;
extern crate chrono;
extern crate tokio_signal;

#[macro_use]
extern crate prometheus;

#[macro_use]
extern crate lazy_static;

mod logger;
mod config;
mod consumer;
mod notifier;
mod events;
mod producer;
mod metrics;
mod certificate_registry;

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
use producer::ResponseProducer;
use futures::sync::{mpsc, oneshot};
use metrics::StatisticsServer;
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
    let mut config_file_location = String::from("./config/config.toml");
    let mut number_of_threads    = 1;

    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Google Firebase Push Notification System");
        ap.refer(&mut config_file_location)
            .add_option(&["-c", "--config"], Store,
                        "Config file (default: ./config/config.toml)");
        ap.refer(&mut number_of_threads)
            .add_option(&["-n", "--number_of_consumers"], Store,
                        "Number of consumer threads");
        ap.parse_args_or_exit();
    }

    let config               = Arc::new(Config::parse(config_file_location));
    let certificate_registry = Arc::new(CertificateRegistry::new(config.clone()));
    let control              = Arc::new(AtomicBool::new(true));

    let mut threads: Vec<JoinHandle<_>> = Vec::new();

    // Prometheus statistics server kill-switch
    let (server_tx, server_rx) = oneshot::channel();

    {
        // Consumer to notifier communication
        let (notifier_tx, notifier_rx) = mpsc::channel(10000);

        // Notifier to response producer communication
        let (producer_tx, producer_rx) = mpsc::channel(10000);

        threads.push({
            let notifier = Notifier::new();

            thread::spawn(move || {
                info!("Starting async notifier thread...");
                notifier.run(notifier_rx, producer_tx);
                info!("Exiting async notifier thread...");
            })
        });

        for i in 0..number_of_threads {
            threads.push({
                let consumer = Consumer::new(
                    control.clone(),
                    config.clone(),
                    certificate_registry.clone());
                let tx = notifier_tx.clone();

                thread::spawn(move || {
                    info!("Starting consumer {}...", i);

                    if let Err(error) = consumer.consume(tx) {
                        error!("Error in consumer: {:?}", error);
                    }

                    info!("Exiting consumer {}...", i);
                })
            });
        }

        threads.push({
            let mut producer = ResponseProducer::new(config.clone());

            thread::spawn(move || {
                info!("Starting response producer thread...");
                producer.run(producer_rx);
                info!("Exiting response producer thread...");
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

        server_tx.complete(());
        control.store(false, Ordering::Relaxed);
        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap();
        }
    }
}
