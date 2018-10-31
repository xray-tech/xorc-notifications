# XORC Notifications

[![Travis Build Status](https://api.travis-ci.org/xray-tech/xorc-notifications.svg?branch=master)](https://travis-ci.org/xray-tech/xorc-notifications)
[![Apache licensed](https://img.shields.io/badge/license-apache-blue.svg)](./LICENSE)

A collection of services consuming [PushNotification
events](https://github.com/xray-tech/xorc-events/blob/master/notification/push_notification.proto)
to send push notifications to apns2/fcm/web push and [Application
events](https://github.com/xray-tech/xorc-events/blob/master/application.proto) to receive configuration.

The systems are by default multi-tenant for sending push notifications to multiple different applications.

- [apns2](src/apns2) for Apple notifications
- [fcm](src/fcm) for Google notificiations
- [web_push](src/web_push) for Web push notifications
- [http_requester](src/http_requester) for generic HTTP requests
- [common](src/common) a library used by all four consumers

## Dependencies

The systems are written with Rust and it should always be possible to compile
with the latest stable version. The de-facto way of getting the latest Rust is
with [rustup](https://rustup.rs/):

```bash
> curl https://sh.rustup.rs -sSf | sh
> rustup update
> rustup default stable
```

To check that everything works:

```bash
> rustc --version
rustc 1.30.0 (da5f414c2 2018-10-24)
> cargo --version
cargo 1.30.0 (36d96825d 2018-10-24)
```

Some of the crates used in the project have dependencies to certain system
libraries and tools, for Ubuntu 18.04 you get them with:

```bash
> sudo apt install build-essential libssl-dev automake ca-certificates libffi-dev protobuf-compiler
```

## Development setup

The project uses [Protocol
Buffers](https://developers.google.com/protocol-buffers/) for event schemas.
`cargo build` should generate the corresponding Rust structs to be used in the
code. By default the protobuf classes are included as a submodule, which must be
imported to the project tree:

```bash
> git submodule update --init
```

Configuration examples for all the consumers are in [config](config/). Create a
copy from an example config removing the ending, and modify it to suit your test
setup.

Running apns2:

```bash
> env CONFIG=./config/apns2.toml cargo run --bin apns2
```

Running fcm:

```bash
> env CONFIG=./config/fcm.toml cargo run --bin fcm
```

Running web_push:

```bash
> env CONFIG=./config/web_push.toml cargo run --bin web_push
```

Running http_requester:

```bash
> env CONFIG=./config/http_requester.toml cargo run --bin http_requester
```

## Example scripts for testing purposes

The [examples](examples/) directory contains helper scripts for testing the
consumers.

To build them:

```bash
cargo build --release --examples
```

The executables are in `target/release` directory.

## Configuration
The system is configuration is handled through a
[toml](https://github.com/toml-lang/toml) file and a environment variable.

### Environment variables

variable     | description                         | example
-------------|-------------------------------------|----------------------------------
`CONFIG`     | The configuration file location     | `/etc/xorc-notifications/config.toml`
`LOG_FORMAT` | Log output format                   | `text` or `json`, default: `text`
`RUST_ENV`   | The program environment             | `test`, `development`, `staging` or `production`, default: `development`

### Required options

section   | key             | description                                | example
----------|-----------------|--------------------------------------------|----------------------------------
`[kafka]` | `input_topic`   | Notification input topic                   | `"production.notifications.apns"`
`[kafka]` | `config_topic`  | Application configuration topic            | `"production.applications"`
`[kafka]` | `output_topic`  | Notification response topic                | `"production.oam"`
`[kafka]` | `group_id`      | Consumer group ID                          | `"production.consumers.apns"`
`[kafka]` | `brokers`       | Comma-separated list of Kafka brokers      | `"kafka1:9092,kafka2:9092"`
`[kafka]` | `consumer_type` | Decides the input protobuf deserialization | `push_notification` for `PushNotification`, `http_request` for `HttpRequest`

### Code Architecture

- All four systems use an asynchronous Kafka consumer consuming the `input_topic`,
  requesting the external service with a client, parsing the response and
  responding back to the caller.
- System should implement the `EventHandler`
  ([request_consumer.rs](src/common/kafka/request_consumer.rs)) and respond with
  `ResponseProducer`
  ([response_producer.rs](src/common/kafka/response_producer.rs)).
- Consumer should keep track of connections for different applications using
  the configuration values from the `config_topic`.
- In general none of the main code should never block.
- All consumers talk HTTP and when requested, return Prometheus statistics
