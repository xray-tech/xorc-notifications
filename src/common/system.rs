use chan_signal::{notify, Signal};
use crate::config::Config;
use crate::kafka::EventHandler;
use crate::kafka::RequestConsumer;
use crate::metrics::StatisticsServer;
use std::{thread, thread::JoinHandle, sync::Arc};
use futures::sync::oneshot;
use crate::logger::Logger;
use slog_scope;

pub struct System;

impl System {
    /// Starts the push system for given handler and configuration.
    pub fn start<H>(name: &'static str, handler: H, config: &Config)
    where
        H: EventHandler + Send + Sync + 'static,
    {
        let exit_signal = notify(&[Signal::INT, Signal::TERM]);
        let (server_tx, server_rx) = oneshot::channel();
        let (request_tx, request_rx) = oneshot::channel();
        let (config_tx, config_rx) = oneshot::channel();

        let logger = Logger::build(name);
        let _log_guard = slog_scope::set_global_logger(logger);

        slog_scope::scope(&slog_scope::logger().new(slog_o!()), || {
            info!("Bringing up the system");

            let mut threads: Vec<JoinHandle<_>> = Vec::new();
            let consumer = Arc::new(RequestConsumer::new(handler, &config.kafka));

            threads.push({
                let consumer = consumer.clone();
                thread::spawn(move || {
                    info!("Starting the request consumer");

                    if let Err(error) = consumer.handle_requests(request_rx) {
                        error!("Error in request consumer"; "error" => format!("{:?}", error));
                    }

                    info!("Exiting request consumer");
                })
            });

            threads.push({
                let consumer = consumer.clone();
                thread::spawn(move || {
                    info!("Starting the config consumer");

                    if let Err(error) = consumer.handle_configs(config_rx) {
                        error!("Error in config consumer"; "error" => format!("{:?}", error));
                    }

                    info!("Exiting config consumer");
                })
            });

            threads.push({
                thread::spawn(move || {
                    info!("Starting statistics server");
                    StatisticsServer::handle(server_rx);
                    info!("Exiting statistics server");
                })
            });

            chan_select! {
                exit_signal.recv() -> signal => {
                    info!("Received signal"; "signal" => format!("{:?}", signal));

                    server_tx.send(()).unwrap();
                    request_tx.send(()).unwrap();
                    config_tx.send(()).unwrap();

                    for thread in threads {
                        thread.thread().unpark();
                        thread.join().unwrap();
                    }
                },
            }
        })
    }
}
