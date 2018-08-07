# XORC Notifications

Systems talking to external services, getting input from and answering back to
Kafka.

- [apns2](src/apns2) for Apple notifications
- [fcm](src/fcm) for Google notificiations
- [web_push](src/web_push) for Web push notifications
- [http_requester](src/http_requester) for generic HTTP requests
- [common](src/common) a library used by all four consumers

## Dependencies

XORC Notifications are written with Rust and should always be possible to compile
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
rustc 1.26.0 (a77568041 2018-05-07)
> cargo --version
cargo 1.26.0 (0e7c5a931 2018-04-06)
```

Some of the crates used in the project have dependencies to certain system
libraries and tools, for Ubuntu 18.04 you get them with:

```bash
> sudo apt install build-essential libssl-dev automake ca-certificates libffi-dev protobuf-compiler
```

## Development setup

The project uses [Protocol
Buffers](https://developers.google.com/protocol-buffers/) for event schemas.
Building the project should generate the corresponding Rust structs to be used
in the code. By default the protobuf classes are included as a submodule, which
must be imported to the project tree:

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

## Configuration
The system is configuration is handled through a
[toml](https://github.com/toml-lang/toml) file and a environment variable.

### Environment variables

variable     | description                         | example
-------------|-------------------------------------|----------------------------------
`CONFIG`     | The configuration file location     | `/etc/xorc-gateway/config.toml`
`LOG_FORMAT` | Log output format                   | `text` or `json`, default: `text`
`RUST_ENV`   | The program environment             | `test`, `development`, `staging` or `production`, default: `development`

### Required options

section   | key             | description                                | example
----------|-----------------|--------------------------------------------|----------------------------------
`[log]`   | `host`          | Graylog address                            | `"graylog.service.consul:12201"`
`[kafka]` | `input_topic`   | Notification input topic                   | `"production.notifications.apns"`
`[kafka]` | `config_topic`  | Application configuration topic            | `"production.crm.applications"`
`[kafka]` | `output_topic`  | Notification response topic                | `"production.oam"`
`[kafka]` | `group_id`      | Consumer group ID                          | `"producction.consumers.apns"`
`[kafka]` | `brokers`       | Comma-separated list of Kafka brokers      | `"kafka1:9092,kafka2:9092"`
`[kafka]` | `consumer_type` | Decides the input protobuf deserialization | `push_notification` for `PushNotification`, `http_request` for `HttpRequest`

### Code Architecture

- All four systems use a asynchronous Kafka consumer consuming the `input_topic`,
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
