use argparse::{ArgumentParser, Store};
use chan_signal::{notify, Signal};
use config::Config;
use kafka::EventHandler;
use kafka::PushConsumer;
use metrics::StatisticsServer;

use std::{thread, thread::JoinHandle};

use futures::sync::oneshot;

pub struct System;

impl System {
    pub fn start<H>(name: &str, handler: H, config: &Config)
    where
        H: EventHandler + Send + 'static,
    {
        let exit_signal = notify(&[Signal::INT, Signal::TERM]);
        let (server_tx, server_rx) = oneshot::channel();
        let (consumer_tx, consumer_rx) = oneshot::channel();
        let mut consumer_partition = 1;

        {
            let mut ap = ArgumentParser::new();
            ap.set_description(name);
            ap.refer(&mut consumer_partition).add_option(
                &["-p", "--partition"],
                Store,
                "Kafka partition to consume, (default: 1)",
            );
            ap.parse_args_or_exit();
        }

        info!("{} starting up!", name);

        let mut threads: Vec<JoinHandle<_>> = Vec::new();

        threads.push({
            let mut consumer = PushConsumer::new(handler, &config.kafka, consumer_partition);

            thread::spawn(move || {
                debug!("Starting consumer...");

                if let Err(error) = consumer.consume(consumer_rx) {
                    error!("Error in consumer: {:?}", error);
                }

                debug!("Exiting consumer...");
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
}
