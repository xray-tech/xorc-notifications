use rdkafka::{
    Message,
    config::ClientConfig,
    consumer::{Consumer, stream_consumer::StreamConsumer, CommitMode},
    topic_partition_list::{TopicPartitionList, Offset},
};

use std::{
    collections::HashMap,
};

use events::push_notification::PushNotification;
use futures::{Future, sync::oneshot, Stream};
use protobuf::parse_from_bytes;
use kafka::{Config, OffsetCounter};
use tokio_core::reactor::Core;

pub trait EventHandler {
    fn handle_notification(&self, event: PushNotification) ->
        Box<Future<Item=(), Error=()> + 'static + Send>;

    fn handle_config(&mut self, payload: &[u8]);
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
    pub fn new(
        event_handler: H,
        config: &Config,
        partition: i32,
    ) -> PushConsumer<H>
    {
        PushConsumer {
            config_topic: config.config_topic.clone(),
            input_topic: config.input_topic.clone(),
            group_id: config.group_id.clone(),
            brokers: config.brokers.clone(),
            partition: partition,
            handler: event_handler,
        }
    }

    pub fn consume(
        &mut self,
        control: oneshot::Receiver<()>,
    ) -> Result<(), ()>
    {
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

        topic_map.insert(
            (self.config_topic.clone(), 1),
            Offset::Beginning
        );

        topic_map.insert(
            (self.input_topic.clone(), self.partition),
            Offset::Stored
        );

        let partitions = TopicPartitionList::from_topic_map(&topic_map);
        consumer.assign(&partitions).expect("Can't subscribe to specified topics");

        let mut offset_counter = OffsetCounter::new(&consumer);

        let processed_stream = consumer.start()
            .filter_map(|result| {
                match result {
                    Ok(msg) => Some(msg),
                    Err(e) => {
                        warn!("Error while receiving from Kafka: {:?}", e);
                        None
                    }
                }
            }).for_each(|msg| {
                match msg.topic() {
                    t if t == self.input_topic => {
                        if let Err(e) = offset_counter.try_store_offset(&msg) {
                            warn!("Error storing offset: #{}", e);
                        }

                        let event_parsing = parse_from_bytes::<PushNotification>(
                            msg.payload().unwrap()
                        );

                        if let Ok(event) = event_parsing {
                            let notification_handling = self.handler.handle_notification(event);
                            handle.spawn(notification_handling);
                        } else {
                            error!("Error parsing protobuf");
                        }
                    },
                    t if t == self.config_topic => {
                        self.handler.handle_config(msg.payload().unwrap());
                    },
                    t => {
                        error!("Unsupported topic: {}", t);
                    }
                }

                Ok(())
            });

        let _ = core.run(
            processed_stream
                .select2(control)
                .then(|_| consumer.commit_consumer_state(CommitMode::Sync))
        );

        Ok(())
    }
}
