use rdkafka::{
    Message,
    message::BorrowedMessage,
    config::ClientConfig,
    consumer::{CommitMode, Consumer, stream_consumer::StreamConsumer},
    topic_partition_list::{Offset, TopicPartitionList},
};
use kafka::Config;
use events::{
    application::Application,
    push_notification::PushNotification,
    http_request::HttpRequest,
    rpc_decoder::RequestWrapper,
};
use futures::{Future, Stream, sync::oneshot};
use protobuf::parse_from_bytes;
use tokio::{self, runtime::current_thread::Runtime};
use regex::Regex;

lazy_static! {
    static ref APP_KEY_RE: Regex =
        Regex::new(
            r"application|([A-Z0-9]{8}-[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{12})"
        ).unwrap();
}

pub trait EventHandler {
    /// True if the consumer should accept the incoming event.
    fn accepts(&self, event: &PushNotification) -> bool;

    /// Try to send a push notification. If key parameter is set, the response
    /// will be sent with the same routing key.
    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send>;

    /// Try to send a http request. If key parameter is set, the response
    /// will be sent with the same routing key.
    fn handle_http(
        &self,
        key: Option<Vec<u8>>,
        event: HttpRequest,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send>;

    /// Handle tenant configuration for connection setup.
    fn handle_config(
        &self,
        id: &str,
        config: Option<Application>
    );
}

pub struct RequestConsumer<H: EventHandler + Send + Sync + 'static> {
    config_topic: String,
    input_topic: String,
    group_id: String,
    brokers: String,
    handler: H,
}

impl<H: EventHandler + Send + Sync + 'static> RequestConsumer<H> {
    /// A kafka consumer to consume push notification events. `EventHandler`
    /// should contain the business logic.
    pub fn new(handler: H, config: &Config) -> RequestConsumer<H> {
        RequestConsumer {
            config_topic: config.config_topic.clone(),
            input_topic: config.input_topic.clone(),
            group_id: config.group_id.clone(),
            brokers: config.brokers.clone(),
            handler,
        }
    }

    /// Consuming the configuration topic for tenant connection setup. A message through `control` stops the consumer.
    pub fn handle_configs(&self, control: oneshot::Receiver<()>) -> Result<(), ()> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", &self.group_id)
            .set("bootstrap.servers", &self.brokers)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest")
            .set("enable.partition.eof", "false")
            .create()
            .expect("Consumer creation failed");

        let mut partitions = TopicPartitionList::new();

        for partition in 0..12 {
            partitions.add_partition_offset(
                &self.config_topic,
                partition,
                Offset::Beginning
            );
        }

        consumer.assign(&partitions).expect("Can't subscribe to specified topics");

        info!("Starting config processing");

        self.handler(consumer, control, &|msg: BorrowedMessage| {
            let convert_key = msg.key().and_then(|key| {
                String::from_utf8(key.to_vec()).ok()
            });

            match convert_key {
                Some(ref key) if APP_KEY_RE.is_match(key) => {
                    let type_parsing = msg.payload().and_then(|payload| {
                        parse_from_bytes::<RequestWrapper>(&payload).ok()
                    });

                    let application_id: &str = key
                        .split('|')
                        .collect::<Vec<&str>>()[1];

                    match type_parsing {
                        Some(ref decoder) => {
                            match decoder.get_header().get_field_type() {
                                "application.Application" => {
                                    debug!(
                                        "Got application configuration";
                                        "universe" => application_id,
                                        "key" => key
                                    );

                                    self.handle_config(application_id, Some(&msg))
                                }
                                t =>
                                    debug!("Invalid type: {}", t),
                            }
                        }
                        None => {
                            debug!(
                                "Got null configuration";
                                "universe" => application_id,
                                "key" => key
                            );

                            self.handle_config(application_id, None);
                        }
                    }
                }
                _ => debug!("Not an application configuration here")
            }

            Ok(())
        })
    }

    /// Consume until event is sent through `control`.
    pub fn handle_requests(&self, control: oneshot::Receiver<()>) -> Result<(), ()> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", &self.group_id)
            .set("bootstrap.servers", &self.brokers)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "latest")
            .set("enable.partition.eof", "false")
            .create()
            .expect("Consumer creation failed");

        consumer.subscribe(&[&self.input_topic]).expect("Can't subscribe to specified topics");

        info!("Starting events processing");

        self.handler(consumer, control, &|msg: BorrowedMessage| {
            debug!(
                "Got message";
                "topic" => msg.topic(),
                "key" => msg.key().and_then(|key| String::from_utf8(key.to_vec()).ok())
            );

            let type_parsing = msg.payload()
                .and_then(|payload| parse_from_bytes::<RequestWrapper>(&payload).ok());

            match type_parsing {
                Some(ref decoder) => {
                    match decoder.get_header().get_field_type() {
                        "notification.PushNotification" =>
                            self.handle_push(&msg),
                        "http.HttpRequest" =>
                            self.handle_http(&msg),
                        t =>
                            debug!("Invalid type: {}", t),

                    }
                }
                None => {
                    error!("Invalid RPC request");
                }
            }

            Ok(())
        })
    }

    fn handler(
        &self,
        consumer: StreamConsumer,
        control: oneshot::Receiver<()>,
        process_event: &Fn(BorrowedMessage) -> Result<(), ()>
    ) -> Result<(), ()> {
        let mut core = Runtime::new().unwrap();

        let processed_stream = consumer
            .start()
            .filter_map(|result| match result {
                Ok(msg) => Some(msg),
                Err(e) => {
                    warn!("Error while receiving from Kafka: {:?}", e);
                    None
                }
            })
            .for_each(|msg| process_event(msg))
            .select2(control)
            .then(|_| consumer.commit_consumer_state(CommitMode::Sync));

        core.block_on(processed_stream).unwrap();

        Ok(())
    }

    fn handle_push(&self, msg: &BorrowedMessage) {
        let event_parsing = msg.payload()
            .and_then(|payload| parse_from_bytes::<PushNotification>(payload).ok());

        match event_parsing {
            Some(event) => {
                if self.handler.accepts(&event) {
                    let notification_handling = self.handler.handle_notification(
                        msg.key().map(|key| key.to_vec()),
                        event
                    );

                    tokio::spawn(notification_handling);
                } else {
                    debug!("Push notification skipped");
                }
            }
            None => {
                error!("Error parsing a PushNotification event");
            }
        }
    }

    fn handle_http(&self, msg: &BorrowedMessage) {
        let event_parsing = msg.payload()
            .and_then(|payload| parse_from_bytes::<HttpRequest>(payload).ok());

        match event_parsing {
            Some(event) => {
                let http_handling = self.handler.handle_http(
                    msg.key().map(|key| key.to_vec()),
                    event
                );

                tokio::spawn(http_handling);
            }
            None => {
                debug!("Not a HttpRequest event this one here");
            }
        }
    }

    fn handle_config(&self, msg_id: &str, msg: Option<&BorrowedMessage>) {
        let event = msg
            .and_then(|msg| msg.payload())
            .and_then(|payload| parse_from_bytes::<Application>(&payload).ok());

        self.handler.handle_config(msg_id, event);
    }
}
