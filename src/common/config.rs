use kafka;
use toml;

use std::{fs::File, io::prelude::*};

#[derive(Deserialize, Debug)]
pub struct Config {
    pub kafka: kafka::Config,
}

impl Config {
    /// Load TOML-formatted configuration from `path`.
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
