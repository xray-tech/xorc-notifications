use std::sync::Arc;
use tokio_core::reactor::Core;
use tokio_core::net::TcpStream;
use lapin::client::{Client, ConnectionOptions};
use lapin::channel::*;
use lapin::types::FieldTable;
use apns2::error::Error;
use events::push_notification::PushNotification;
use protobuf::parse_from_bytes;
use config::Config;
use certificate_registry::Application;
use logger::{GelfLogger, LogAction};
use gelf::{Level as GelfLevel, Message as GelfMessage};
use futures::{Future, Sink, Stream};
use futures::future::{ok, Shared};
use futures::sync::mpsc::Sender;
use std::thread;
use std::net::{SocketAddr, ToSocketAddrs};
use notifier::Notifier;
use metrics::*;
use certificate_registry::ApnsConnectionParameters;
use futures::sync::oneshot;
use producer::ApnsData;
use consumer_supervisor::{CONSUMER_FAILURES, MAX_FAILURES};

pub struct Consumer {
    config: Arc<Config>,
    logger: Arc<GelfLogger>,
    application: Application,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        match self.application.connection_parameters {
            ApnsConnectionParameters::Certificate {
                pkcs12: _,
                password: _,
                endpoint: _,
            } => {
                CERTIFICATE_CONSUMERS.dec();
            }
            ApnsConnectionParameters::Token {
                pkcs8: _,
                key_id: _,
                team_id: _,
                endpoint: _,
            } => {
                TOKEN_CONSUMERS.dec();
            }
        };
    }
}

impl Consumer {
    pub fn new(config: Arc<Config>, app: Application, logger: Arc<GelfLogger>) -> Consumer {
        match app.connection_parameters {
            ApnsConnectionParameters::Certificate {
                pkcs12: _,
                password: _,
                endpoint: _,
            } => {
                CERTIFICATE_CONSUMERS.inc();
            }
            ApnsConnectionParameters::Token {
                pkcs8: _,
                key_id: _,
                team_id: _,
                endpoint: _,
            } => {
                TOKEN_CONSUMERS.inc();
            }
        }

        Consumer {
            application: app,
            config: config,
            logger: logger,
        }
    }

    pub fn consume(
        &self,
        control: Shared<oneshot::Receiver<()>>,
        tx_producer: Sender<ApnsData>,
    ) -> Result<(), Error> {
        let address: SocketAddr = format!(
            "{}:{}",
            self.config.rabbitmq.host, self.config.rabbitmq.port
        ).to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();

        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let app_id = self.application.id;
        let connection_parameters = self.application.connection_parameters.clone();
        let notifier = Notifier::new(&handle, connection_parameters, self.application.topic.clone())?;
        let queue_name = format!("{}_{}", self.config.rabbitmq.queue, self.application.id);
        let routing_key = format!(
            "{}_{}",
            self.config.rabbitmq.routing_key, self.application.id
        );

        let connection_options = ConnectionOptions {
            username: self.config.rabbitmq.login.clone(),
            password: self.config.rabbitmq.password.clone(),
            vhost: self.config.rabbitmq.vhost.clone(),
            ..Default::default()
        };

        let connecting = TcpStream::connect(&address, &handle)
            .and_then(move |stream| Client::connect(stream, &connection_options))
            .and_then(|(client, heartbeat_future_fn)| {
                let heartbeat_client = client.clone();
                thread::Builder::new()
                    .name("heartbeat thread".to_string())
                    .spawn(move || {
                        let heartbeat_core = Core::new()
                            .unwrap()
                            .run(heartbeat_future_fn(&heartbeat_client));

                        match heartbeat_core {
                            Ok(_) => info!("Consumer #{} hearbeat core exited successfully", app_id),
                            Err(e) => {
                                error!("Consumer #{} heartbeat core crashed, restarting... ({:?})", app_id, e);
                                let mut failures = CONSUMER_FAILURES.lock().unwrap();
                                failures.insert(app_id, MAX_FAILURES);
                            }
                        }
                    })
                    .unwrap();

                client.create_channel()
            });

        let env_declaring = move |channel: Channel<TcpStream>| {
            channel.exchange_declare(
                &self.config.rabbitmq.exchange,
                &self.config.rabbitmq.exchange_type,
                &ExchangeDeclareOptions {
                    passive: false,
                    durable: true,
                    auto_delete: false,
                    internal: false,
                    nowait: false,
                    ..Default::default()
                },
                &FieldTable::new(),
            ).and_then(move |_| {
                channel.exchange_declare(
                    &self.config.rabbitmq.response_exchange,
                    &self.config.rabbitmq.response_exchange_type,
                    &ExchangeDeclareOptions {
                        passive: false,
                        durable: true,
                        auto_delete: false,
                        internal: false,
                        nowait: false,
                        ..Default::default()
                    },
                    &FieldTable::new(),
                ).and_then(move |_| {
                    channel.queue_declare(
                        &queue_name,
                        &QueueDeclareOptions {
                            passive: false,
                            durable: true,
                            exclusive: false,
                            auto_delete: false,
                            nowait: false,
                            ..Default::default()
                        },
                        &FieldTable::new(),
                    ).and_then(move |_| {
                        channel.queue_bind(
                            &queue_name,
                            &self.config.rabbitmq.exchange,
                            &routing_key,
                            &QueueBindOptions {
                                nowait: false,
                                ..Default::default()
                            },
                            &FieldTable::new(),
                        )
                    })
                })
            })
        };

        let consuming = connecting.and_then(move |channel| {
            let consumer_tag = format!("apns_consumer_{}", &self.application.id);
            let queue = format!("{}_{}", &self.config.rabbitmq.queue, &self.application.id);

            env_declaring(channel.clone()).and_then(move |_| {
                channel
                    .basic_consume(&queue, &consumer_tag,
                        &BasicConsumeOptions {
                            no_local: true,
                            no_ack: true,
                            exclusive: false,
                            no_wait: false,
                            ..Default::default()
                        }, &FieldTable::new())
                    .and_then(move |stream| {
                        let mut log_msg = GelfMessage::new(String::from("Consumer started"));

                        log_msg.set_full_message(String::from(
                            "Consumer is created and will now start consuming messages",
                        ));
                        log_msg.set_level(GelfLevel::Informational);

                        let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerStart));
                        let _ = log_msg.set_metadata("app_id", format!("{}", &self.application.id));
                        let _ = log_msg.set_metadata("consumer_name", format!("{}", &consumer_tag));
                        let _ = log_msg.set_metadata("queue", format!("{}", &queue));

                        self.logger.log_message(log_msg);

                        let work_loop = stream.for_each(move |message| {
                            if let Ok(event) = parse_from_bytes::<PushNotification>(&message.data) {
                                let tx = tx_producer.clone();

                                REQUEST_COUNTER.with_label_values(&[
                                    "requested", event.get_application_id(), event.get_campaign_id()
                                ]).inc();

                                CALLBACKS_INFLIGHT.inc();
                                let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

                                let work = notifier
                                    .notify(&event)
                                    .then(move |result| {
                                        timer.observe_duration();
                                        CALLBACKS_INFLIGHT.dec();
                                        tx.send((event, result))
                                    })
                                    .then(|_| ok(()));

                                handle.spawn(work);
                            } else {
                                error!("Error parsing protobuf");
                            }

                            Ok(())
                        });

                        work_loop.select2(control).then(move |_| {
                            channel.close(200, "Bye");
                            Ok(())
                        })
                    })
            })
        });

        let _ = core.run(consuming);
        Ok(())
    }
}
