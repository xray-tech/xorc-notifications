use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub postgres: PostgresConfig,
    pub kafka: KafkaConfig,
    pub log: LogConfig,
}

impl Config {
    pub fn parse(path: &str) -> Config {
        let mut config_toml = String::new();

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                panic!("Error while reading config file: [{}]", err);
            }
        };

        file.read_to_string(&mut config_toml)
            .unwrap_or_else(|err| panic!("Error while reading config: [{}]", err));

        toml::from_str(&config_toml).unwrap()
    }
}

#[derive(Deserialize, Debug)]
pub struct KafkaConfig {
    pub input_topic: String,
    pub config_topic: String,
    pub output_topic: String,
    pub retry_topic: String,
    pub group_id: String,
    pub brokers: String,
}

#[derive(Deserialize, Debug)]
pub struct PostgresConfig {
    pub uri: String,
    pub pool_size: u32,
    pub min_idle: u32,
    pub idle_timeout: u64,
    pub max_lifetime: u64,
}

#[derive(Deserialize, Debug)]
pub struct LogConfig {
    pub host: String,
}
