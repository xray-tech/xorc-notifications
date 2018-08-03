use rdkafka::{
    Message,
    config::ClientConfig,
    consumer::{CommitMode, Consumer, stream_consumer::StreamConsumer},
    topic_partition_list::{Offset, TopicPartitionList},
};
use std::collections::HashMap;
use kafka::{Config, ConsumerType, offset_counter::OffsetCounter};
use events::{
    crm::Application,
    push_notification::PushNotification,
    http_request::HttpRequest,
    header_decoder::HeaderWrapper,
};
use futures::{Future, Stream, sync::oneshot};
use protobuf::parse_from_bytes;
use tokio::{self, executor::current_thread::CurrentThread};

pub trait EventHandler {
    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send>;

    fn handle_http(
        &self,
        key: Option<Vec<u8>>,
        event: HttpRequest,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send>;

    fn handle_config(&mut self, config: Application);
}

pub struct RequestConsumer<H: EventHandler> {
    consumer_type: ConsumerType,
    config_topic: String,
    input_topic: String,
    group_id: String,
    brokers: String,
    partition: i32,
    handler: H,
}

impl<H: EventHandler> RequestConsumer<H> {
    /// A kafka consumer to consume push notification events. `EventHandler`
    /// should contain the business logic.
    pub fn new(handler: H, config: &Config, partition: i32) -> RequestConsumer<H> {
        RequestConsumer {
            consumer_type: config.consumer_type.clone(),
            config_topic: config.config_topic.clone(),
            input_topic: config.input_topic.clone(),
            group_id: config.group_id.clone(),
            brokers: config.brokers.clone(),
            partition,
            handler,
        }
    }

    /// Consume until event is sent through `control`.
    pub fn consume(&mut self, control: oneshot::Receiver<()>) -> Result<(), ()> {
        let mut core = CurrentThread::new();

        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", &self.group_id)
            .set("bootstrap.servers", &self.brokers)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "latest")
            .set("enable.partition.eof", "false")
            .create()
            .expect("Consumer creation failed");

        let mut topic_map = HashMap::new();

        topic_map.insert((self.config_topic.clone(), 1), Offset::Beginning);

        topic_map.insert((self.input_topic.clone(), self.partition), Offset::Stored);

        let partitions = TopicPartitionList::from_topic_map(&topic_map);
        consumer
            .assign(&partitions)
            .expect("Can't subscribe to specified topics");

        let mut offset_counter = OffsetCounter::new(&consumer);

        let processed_stream = consumer
            .start()
            .filter_map(|result| match result {
                Ok(msg) => Some(msg),
                Err(e) => {
                    warn!("Error while receiving from Kafka: {:?}", e);
                    None
                }
            })
            .for_each(|msg| {
                match msg.topic() {
                    t if t == self.input_topic => {
                        if let Err(e) = offset_counter.try_store_offset(&msg) {
                            warn!("Error storing offset: #{}", e);
                        }

                        match self.consumer_type {
                            ConsumerType::PushNotification => {
                                let event_parsing = msg.payload()
                                    .and_then(|payload| parse_from_bytes::<PushNotification>(payload).ok());

                                match event_parsing {
                                    Some(event) => {
                                        let notification_handling = self.handler.handle_notification(
                                            msg.key().map(|key| key.to_vec()),
                                            event
                                        );

                                        tokio::spawn(notification_handling);
                                    }
                                    None => {
                                        error!("Error parsing notification protobuf");
                                    }
                                }
                            }
                            ConsumerType::HttpRequest => {
                                let event_parsing = msg.payload()
                                    .and_then(|payload| parse_from_bytes::<HttpRequest>(payload).ok());

                                match event_parsing {
                                    Some(event) => {
                                        let notification_handling = self.handler.handle_http(
                                            msg.key().map(|key| key.to_vec()),
                                            event
                                        );

                                        tokio::spawn(notification_handling);
                                    }
                                    None => {
                                        error!("Error parsing http protobuf");
                                    }
                                }
                            }
                        }
                    }
                    t if t == self.config_topic => {
                        let event_parsing = msg.payload()
                            .and_then(|payload| parse_from_bytes::<HeaderWrapper>(&payload).ok());

                        match event_parsing {
                            Some(ref decoder) if decoder.get_header().get_field_type() == "crm.Application" => {
                                let event_parsing = msg.payload()
                                    .and_then(|payload| parse_from_bytes::<Application>(&payload).ok());

                                match event_parsing {
                                    Some(event) =>
                                        self.handler.handle_config(event),
                                    None =>
                                        error!("Error parsing protobuf event")
                                }

                            }
                            _ => {
                                debug!("Not an application we have over here...");
                            }
                        }
                    }
                    t => {
                        error!("Unsupported topic: {}", t);
                    }
                }

                Ok(())
            }).select2(control).then(|_| consumer.commit_consumer_state(CommitMode::Sync));

        let _ = core.block_on(processed_stream);

        Ok(())
    }
}
