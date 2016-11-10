use std::fs::File;
use std::io::prelude::*;
use toml::{Parser, Value};
use toml;

#[derive(RustcEncodable, RustcDecodable, Debug)]
pub struct Config {
    pub postgres: PostgresConfig,
    pub rabbitmq: RabbitMqConfig,
    pub metrics: MetricsConfig,
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

        let mut parser = Parser::new(&config_toml);
        let toml = parser.parse();

        if toml.is_none() {
            for err in &parser.errors {
                let (loline, locol) = parser.to_linecol(err.lo);
                let (hiline, hicol) = parser.to_linecol(err.hi);
                println!("{}:{}:{}-{}:{} error: {}",
                         path, loline, locol, hiline, hicol, err.desc);
            }
            panic!("Exiting server");
        }

        let config = Value::Table(toml.unwrap());

        toml::decode(config).unwrap()
    }
}

#[derive(RustcEncodable, RustcDecodable, Debug)]
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

#[derive(RustcEncodable, RustcDecodable, Debug)]
pub struct MetricsConfig {
    pub uri: String,
    pub database: String,
    pub tick_duration: u64,
    pub login: String,
    pub password: String,
    pub application: String,
}

#[derive(RustcEncodable, RustcDecodable, Debug)]
pub struct PostgresConfig {
    pub uri: String,
    pub pool_size: u32,
    pub min_idle: u32,
    pub idle_timeout: u64,
    pub max_lifetime: u64,
}
