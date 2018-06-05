use rdkafka::{config::ClientConfig, producer::{DeliveryFuture, FutureProducer}};

use kafka::Config;

use events::{ResponseAction, header::Header, push_notification::PushNotification,
             push_result::PushResult};

use chrono::offset::Utc;
use protobuf::Message;
use std::sync::Arc;

struct Kafka {
    output_topic: String,
    producer: FutureProducer,
}

pub struct ResponseProducer {
    kafka: Arc<Kafka>,
}

impl ResponseProducer {
    pub fn new(config: &Config) -> ResponseProducer {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("produce.offset.report", "true")
            .create()
            .expect("Producer creation error");

        let kafka = Arc::new(Kafka {
            output_topic: config.output_topic.clone(),
            producer: producer,
        });

        ResponseProducer { kafka }
    }

    fn get_retry_after(event: &PushNotification) -> u32 {
        if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        }
    }

    pub fn publish(
        &self,
        event: PushNotification,
        response_action: ResponseAction,
    ) -> DeliveryFuture {
        let mut result_event = PushResult::new();

        let mut header = Header::new();
        header.set_created_at(Utc::now().timestamp_millis());
        header.set_source(String::from("apns"));
        header.set_recipient_id(String::from(event.get_header().get_recipient_id()));
        header.set_field_type(String::from("notification.NotificationResult"));

        result_event.set_header(header);

        match response_action {
            ResponseAction::None => result_event.set_none(true),
            ResponseAction::UnsubscribeEntity => result_event.set_unsubscribe_entity(true),
            ResponseAction::Retry => result_event.set_retry_in(Self::get_retry_after(&event)),
        }

        result_event.set_notification(event);

        self.kafka.producer.send_copy::<Vec<u8>, str>(
            &self.kafka.output_topic,
            None,                                               // topic
            Some(&result_event.write_to_bytes().unwrap()),      // payload
            Some(result_event.get_header().get_recipient_id()), // key
            None,                                               // timestamp
            1000, // block in milliseconds if the queue is full,
        )
    }
}

impl Clone for ResponseProducer {
    fn clone(&self) -> Self {
        ResponseProducer {
            kafka: self.kafka.clone(),
        }
    }
}
