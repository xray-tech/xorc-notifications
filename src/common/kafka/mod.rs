mod offset_counter;
mod push_consumer;

pub use self::push_consumer::{PushConsumer, EventHandler};

#[derive(Deserialize, Debug)]
pub struct Config {
    pub input_topic: String,
    pub config_topic: String,
    pub output_topic: String,
    pub retry_topic: String,
    pub group_id: String,
    pub brokers: String,
}

