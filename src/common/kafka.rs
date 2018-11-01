mod request_consumer;
mod response_producer;

pub use self::request_consumer::{EventHandler, RequestConsumer};
pub use self::response_producer::ResponseProducer;
pub use rdkafka::producer::DeliveryFuture;

#[derive(Deserialize, Debug)]
pub struct Config {
    /// Kafka topic for incoming `PushNotification` events, triggering a push
    /// notification to be sent.
    pub input_topic: String,
    /// Kafka topic for incoming `Application` events, holding the tenant
    /// configuration.
    pub config_topic: String,
    /// Kafka topic for push notification responses.
    pub output_topic: String,
    /// Kafka consumer group ID.
    pub group_id: String,
    /// A comma-separated list of Kafka brokers to connect.
    pub brokers: String,
}
