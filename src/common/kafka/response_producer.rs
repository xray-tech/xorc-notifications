use rdkafka::{
    config::ClientConfig,
    producer::future_producer::{
        DeliveryFuture,
        FutureProducer,
        FutureRecord,
    },
};

use kafka::Config;
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
    /// Producer to send responses to notification events.
    pub fn new(config: &Config) -> ResponseProducer {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("produce.offset.report", "true")
            .create()
            .expect("Producer creation error");

        let kafka = Arc::new(Kafka {
            output_topic: config.output_topic.clone(),
            producer,
        });

        ResponseProducer { kafka }
    }

    /// Send the push response. If key is set, sets the routing key in the Kafka
    /// message.
    pub fn publish(
        &self,
        key: Option<Vec<u8>>,
        event: &Message,
    ) -> DeliveryFuture {
        let payload = event.write_to_bytes().unwrap();

        let record = FutureRecord {
            topic: &self.kafka.output_topic,
            partition: None,
            payload: Some(&payload),
            key: key.as_ref(),
            timestamp: None,
            headers: None,
        };

        self.kafka.producer.send::<Vec<u8>, Vec<u8>>(record, -1)
    }
}

impl Clone for ResponseProducer {
    fn clone(&self) -> Self {
        ResponseProducer {
            kafka: self.kafka.clone(),
        }
    }
}
