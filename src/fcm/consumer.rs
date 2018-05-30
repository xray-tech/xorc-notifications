use std::{
    collections::HashMap,
};

use common::{
    metrics::*,
    events::push_notification::PushNotification,
    events::google_config::GoogleConfig,
    kafka::OffsetCounter,
};

use futures::{
    Future,
    Stream,
    sync::oneshot,
    future::ok,
};

use rdkafka::{
    Message,
    config::ClientConfig,
    consumer::{Consumer, stream_consumer::StreamConsumer, CommitMode},
    topic_partition_list::{TopicPartitionList, Offset},
};

use tokio_core::{
    reactor::Core,
};

use notifier::Notifier;
use producer::FcmProducer;
use protobuf::parse_from_bytes;
use hyper::error::Error;
use gelf;

pub struct FcmConsumer {
    producer: FcmProducer,
    api_keys: HashMap<String, String>,
    partition: i32,
}

use ::{CONFIG, GLOG};

impl FcmConsumer {
    pub fn new(partition: i32) -> FcmConsumer {
        let api_keys = HashMap::new();
        let producer = FcmProducer::new();

        FcmConsumer {
            producer,
            api_keys,
            partition,
        }
    }

    pub fn consume(
        &mut self,
        control: oneshot::Receiver<()>,
    ) -> Result<(), Error>
    {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", &CONFIG.kafka.group_id)
            .set("bootstrap.servers", &CONFIG.kafka.brokers)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "latest")
            .set("enable.partition.eof", "false")
            .create()
            .expect("Consumer creation failed");

        let mut topic_map = HashMap::new();
        topic_map.insert((CONFIG.kafka.config_topic.clone(), 1), Offset::Beginning);
        topic_map.insert((CONFIG.kafka.input_topic.clone(), self.partition), Offset::Stored);

        let partitions = TopicPartitionList::from_topic_map(&topic_map);
        consumer.assign(&partitions).expect("Can't subscribe to specified topics");

        let mut offset_counter = OffsetCounter::new(&consumer);
        let notifier = Notifier::new();

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
                    t if t == CONFIG.kafka.input_topic => {
                        if let Err(e) = offset_counter.try_store_offset(&msg) {
                            warn!("Error storing offset: #{}", e);
                        }

                        if let Ok(event) = parse_from_bytes::<PushNotification>(msg.payload().unwrap()) {
                            let producer = self.producer.clone();
                            let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

                            CALLBACKS_INFLIGHT.inc();

                            if let Some(api_key) = self.api_keys.get(event.get_application_id()) {
                                let notification_send = notifier
                                    .notify(&event, api_key)
                                    .then(move |result| {
                                        timer.observe_duration();
                                        CALLBACKS_INFLIGHT.dec();

                                        match result {
                                            Ok(response) =>
                                                producer.handle_response(
                                                    event,
                                                    response
                                                ),
                                            Err(error) =>
                                                producer.handle_error(
                                                    event,
                                                    error
                                                ),
                                        }
                                    })
                                    .then(|_| ok(()));

                                handle.spawn(notification_send);
                            } else {
                                producer.handle_no_cert(event);
                            }
                        } else {
                            error!("Error parsing protobuf");
                        }
                    },
                    t if t == CONFIG.kafka.config_topic => {
                        if let Ok(mut event) = parse_from_bytes::<GoogleConfig>(msg.payload().unwrap()) {
                            self.handle_config(&mut event);
                        } else {
                            error!("Error parsing protobuf");
                        }
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

    fn handle_config(&mut self, event: &mut GoogleConfig) {
        let _ = self.log_config_change("Push config update", &event);

        self.api_keys.insert(
            String::from(event.get_application_id()),
            String::from(event.get_api_key()),
        );
    }

    fn log_config_change(
        &self,
        title: &str,
        event: &GoogleConfig,
    ) -> Result<(), gelf::Error>
    {
        let mut test_msg = gelf::Message::new(String::from(title));

        test_msg.set_metadata("app_id", format!("{}", event.get_application_id()))?;
        test_msg.set_metadata("api_key", format!("{}", event.get_api_key()))?;

        GLOG.log_message(test_msg);

        Ok(())
    }
}
