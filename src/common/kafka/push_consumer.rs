use rdkafka::{Message, config::ClientConfig,
              consumer::{CommitMode, Consumer, stream_consumer::StreamConsumer},
              topic_partition_list::{Offset, TopicPartitionList}};

use std::collections::HashMap;

use kafka::{Config, offset_counter::OffsetCounter};

use events::{application::Application, push_notification::PushNotification};

use futures::{Future, Stream, sync::oneshot};
use protobuf::parse_from_bytes;
use tokio_core::reactor::Core;

pub trait EventHandler {
    fn handle_notification(
        &self,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send>;

    fn handle_config(&mut self, config: Application);
}

pub struct PushConsumer<H: EventHandler> {
    config_topic: String,
    input_topic: String,
    group_id: String,
    brokers: String,
    partition: i32,
    handler: H,
}

impl<H: EventHandler> PushConsumer<H> {
    /// A kafka consumer to consume push notification events. `EventHandler`
    /// should contain the business logic.
    pub fn new(event_handler: H, config: &Config, partition: i32) -> PushConsumer<H> {
        PushConsumer {
            config_topic: config.config_topic.clone(),
            input_topic: config.input_topic.clone(),
            group_id: config.group_id.clone(),
            brokers: config.brokers.clone(),
            partition: partition,
            handler: event_handler,
        }
    }

    /// Consume until event is sent through `control`.
    pub fn consume(&mut self, control: oneshot::Receiver<()>) -> Result<(), ()> {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

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

                        let event_parsing = msg.payload()
                            .and_then(|payload| parse_from_bytes::<PushNotification>(payload).ok());

                        match event_parsing {
                            Some(event) => {
                                let notification_handling = self.handler.handle_notification(event);
                                handle.spawn(notification_handling);
                            }
                            None => {
                                error!("Error parsing notification protobuf");
                            }
                        }
                    }
                    t if t == self.config_topic => {
                        let event_parsing = msg.payload()
                            .and_then(|payload| parse_from_bytes::<Application>(payload).ok());

                        match event_parsing {
                            Some(event) => {
                                self.handler.handle_config(event);
                            }
                            None => {
                                error!("Error parsing notification protobuf");
                            }
                        }
                    }
                    t => {
                        error!("Unsupported topic: {}", t);
                    }
                }

                Ok(())
            });

        let _ = core.run(
            processed_stream
                .select2(control)
                .then(|_| consumer.commit_consumer_state(CommitMode::Sync)),
        );

        Ok(())
    }
}
