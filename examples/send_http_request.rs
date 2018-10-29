extern crate common;
extern crate clap;
extern crate rdkafka;
extern crate chrono;
extern crate protobuf;
extern crate futures;

use rdkafka::{
    config::ClientConfig,
    producer::future_producer::{
        FutureProducer,
        FutureRecord,
    },
};

use common::events::{
    http_request::{HttpRequest, HttpRequest_HttpVerb::*},
    rpc::Request,
};

use protobuf::Message;
use futures::future::Future;
use clap::{Arg, App};
use std::collections::HashMap;

fn is_int(v: String) -> Result<(), String> {
    match v.parse::<u64>() {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("The timeout value is not valid: {:?}", e))
    }
}

fn header_value(v: String) -> Result<(), String> {
    let splitted: Vec<&str> = v.split(": ").collect();

    if splitted.len() == 2 {
        Ok(())
    } else {
        Err(format!("Invalid header value: {}", v))
    }
}

fn main() {
    let matches = App::new("HTTP Request Sender")
        .version("4.20")
        .author("Censhare Techlab")
        .about("Sends HTTP requests through Kafka and http_requester")
        .arg(Arg::with_name("kafka_server")
             .short("s")
             .long("kafka_server")
             .value_name("SERVER:PORT")
             .help("Kafka server to connect to")
             .default_value("localhost:9092")
             .takes_value(true))
        .arg(Arg::with_name("kafka_topic")
             .short("t")
             .long("kafka_topic")
             .value_name("TOPIC")
             .help("Kafka topic to write to")
             .default_value("rpc")
             .takes_value(true))
        .arg(Arg::with_name("request")
             .short("X")
             .long("request")
             .value_name("VERB")
             .help("HTTP verb to use with the request")
             .default_value("GET")
             .takes_value(true))
        .arg(Arg::with_name("body")
             .short("d")
             .long("data")
             .value_name("DATA")
             .help("HTTP POST data"))
        .arg(Arg::with_name("timeout")
             .long("timeout")
             .value_name("MILLIS")
             .help("Maximum time allowed to wait for response")
             .takes_value(true)
             .default_value("2000")
             .validator(is_int))
        .arg(Arg::with_name("headers")
             .long("header")
             .short("H")
             .value_name("HeaderName: HeaderValue")
             .takes_value(true)
             .multiple(true)
             .validator(header_value))
        .arg(Arg::with_name("URI")
             .help("The uri to connect")
             .required(true)
             .index(1))
        .get_matches();

    let kafka_server = matches
        .value_of("kafka_server")
        .unwrap_or("localhost:9092");

    let kafka_topic = matches
        .value_of("kafka_topic")
        .unwrap_or("rpc");

    println!("TOPIC: {}", kafka_topic);

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", kafka_server)
        .set("produce.offset.report", "true")
        .create()
        .expect("Producer creation error");

    let mut header = Request::new();
    header.set_field_type("http.HttpRequest".to_string());

    let mut request = HttpRequest::new();
    request.set_header(header);

    match matches.value_of("request").unwrap_or("GET") {
        "GET" => request.set_request_type(GET),
        "POST" => request.set_request_type(POST),
        "PUT" => request.set_request_type(PUT),
        "DELETE" => request.set_request_type(DELETE),
        "PATCH" => request.set_request_type(PATCH),
        "OPTIONS" => request.set_request_type(OPTIONS),
        v => panic!("Unsupported verb: {}", v),
    }

    request.set_uri(matches.value_of("URI").unwrap().to_string());

    if let Some(body) = matches.value_of("body") {
        request.set_body(body.to_string());
    }

    if let Some(timeout) = matches.value_of("timeout").and_then(|t| t.parse::<u64>().ok()) {
        request.set_timeout(timeout);
    }

    if let Some(headers) = matches.values_of("headers") {
        request.set_headers(headers.fold(HashMap::new(), |mut acc, header| {
            let splitted: Vec<&str> = header
                .split(": ")
                .collect();

            acc.insert(splitted[0].to_string(), splitted[1].to_string());
            acc
        }))
    };

    let payload = request.write_to_bytes().unwrap();

    let record = FutureRecord {
        topic: &kafka_topic,
        partition: None,
        payload: Some(&payload),
        key: None,
        timestamp: None,
        headers: None,
    };

    producer.send::<Vec<u8>, Vec<u8>>(record, -1).wait().unwrap().unwrap();
}
