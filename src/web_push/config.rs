use std::fs::File;
use std::io::prelude::*;
use toml;
use common::kafka;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub log: LogConfig,
    pub kafka: kafka::Config,
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
pub struct LogConfig {
    pub host: String,
}
