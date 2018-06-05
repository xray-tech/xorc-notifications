mod offset_counter;
mod push_consumer;
mod response_producer;

pub use self::push_consumer::{PushConsumer, EventHandler};
pub use self::response_producer::ResponseProducer;
pub use rdkafka::producer::DeliveryFuture;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub input_topic: String,
    pub config_topic: String,
    pub output_topic: String,
    pub group_id: String,
    pub brokers: String,
}

