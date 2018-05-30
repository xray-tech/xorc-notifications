use rdkafka::{
    Message,
    config::ClientConfig,
    consumer::{Consumer, stream_consumer::StreamConsumer, CommitMode},
    topic_partition_list::{TopicPartitionList, Offset},
};

use futures::{
    Future,
    Stream,
    future::ok,
    sync::{
        oneshot,
    },
};

use tokio_core::{
    reactor::Core,
};

use std::{
    collections::HashMap,
};

use common::{
    events::{
        apple_config::*,
        push_notification::PushNotification,
    },
    logger::{
        LogAction
    },
    metrics::*,
    kafka::OffsetCounter,
};

use gelf::{
    Message as GelfMessage,
    Error as GelfError
};

use a2::{
    error::Error,
    client::Endpoint,
};

use protobuf::{parse_from_bytes};
use notifier::Notifier;
use producer::ApnsProducer;

use ::{GLOG, CONFIG};

pub struct ApnsConsumer {
    producer: ApnsProducer,
    notifiers: HashMap<String, Notifier>,
    partition: i32,
}

impl ApnsConsumer {
    pub fn new(partition: i32) -> ApnsConsumer {
        let notifiers = HashMap::new();
        let producer = ApnsProducer::new();

        ApnsConsumer {
            producer,
            notifiers,
            partition,
        }
    }

    pub fn consume(
        &mut self,
        control: oneshot::Receiver<()>,
    ) -> Result<(), Error> {
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

                            if let Some(notifier) = self.notifiers.get(event.get_application_id()) {
                                let notification_send = notifier
                                    .notify(&event)
                                    .then(move |result| {
                                        timer.observe_duration();
                                        CALLBACKS_INFLIGHT.dec();

                                        match result {
                                            Ok(response) =>
                                                producer.handle_ok(event, response),
                                            Err(Error::ResponseError(e)) =>
                                                producer.handle_err(event, e),
                                            Err(e) =>
                                                producer.handle_fatal(event, e),
                                       }
                                    })
                                    .then(|_| ok(()));

                                handle.spawn(notification_send);
                            } else {
                                producer.handle_fatal(event, Error::ConnectionError);
                            }
                        } else {
                            error!("Error parsing protobuf");
                        }
                    },
                    t if t == CONFIG.kafka.config_topic => {
                        if let Ok(mut event) = parse_from_bytes::<AppleConfig>(msg.payload().unwrap()) {
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

    fn handle_config(&mut self, event: &mut AppleConfig) {
        let _ = self.log_config_change("Push config update", &event);
        let application_id = String::from(event.get_application_id());

        let endpoint = match event.get_endpoint() {
            ConnectionEndpoint::Production => Endpoint::Production,
            ConnectionEndpoint::Sandbox => Endpoint::Sandbox,
        };

        let topic = String::from(event.get_apns_topic());

        if event.has_token() {
            let token = event.mut_token();
            let pkcs8 = token.mut_pkcs8().clone();

            let notifier_result = Notifier::token(
                &mut pkcs8.as_slice(),
                token.get_key_id(),
                token.get_team_id(),
                endpoint,
                topic,
            );

            match notifier_result {
                Ok(notifier) => {
                    self.notifiers.insert(application_id, notifier);
                },
                Err(error) => {
                    error!("Error creating a notifier for application #{}: {:?}", application_id, error);
                }
            }
        } else if event.has_certificate() {
            let certificate = event.mut_certificate();
            let pkcs12 = certificate.mut_pkcs12().clone();

            let notifier_result = Notifier::certificate(
                &mut pkcs12.as_slice(),
                certificate.get_password(),
                endpoint,
                topic,
            );

            match notifier_result {
                Ok(notifier) => {
                    self.notifiers.insert(application_id, notifier);
                },
                Err(error) => {
                    error!("Error creating a notifier for application #{}: {:?}", application_id, error);
                }
            }
        } else {
            if let Some(_) = self.notifiers.remove(&application_id) {
                info!("Deleted notifier for application #{}", application_id);
            }
        }
    }

    fn log_config_change(
        &self,
        title: &str,
        event: &AppleConfig,
    ) -> Result<(), GelfError>
    {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_metadata("app_id", format!("{}", event.get_application_id()))?;

        if event.has_token() {
            test_msg.set_metadata("endpoint", format!("{:?}", event.get_endpoint()))?;
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
            test_msg.set_metadata("connection_type", "token".to_string())?;

            let token = event.get_token();

            test_msg.set_metadata("key_id", token.get_key_id().to_string())?;
            test_msg.set_metadata("team_id", token.get_team_id().to_string())?;
        } else if event.has_certificate() {
            test_msg.set_metadata("endpoint", format!("{:?}", event.get_endpoint()))?;
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
            test_msg.set_metadata("connection_type", "certificate".to_string())?;
        } else {
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerDelete))?;
        }

        GLOG.log_message(test_msg);

        Ok(())
    }
}
