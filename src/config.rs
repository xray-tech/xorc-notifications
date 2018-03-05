use std::fs::File;
use std::io::prelude::*;
use toml;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub postgres: PostgresConfig,
    pub rabbitmq: RabbitMqConfig,
    pub log: LogConfig,
}

impl Config {
    pub fn parse(path: String) -> Config {
        let mut config_toml = String::new();

        let mut file = match File::open(&path) {
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
pub struct RabbitMqConfig {
    pub queue: String,
    pub exchange: String,
    pub exchange_type: String,
    pub vhost: String,
    pub host: String,
    pub port: u16,
    pub login: String,
    pub password: String,
    pub routing_key: String,
    pub response_exchange: String,
    pub response_exchange_type: String,
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
