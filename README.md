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

## Example scripts for testing purposes

The [examples](examples/) directory contains helper scripts for testing the
consumers.

To build them:

```bash
cargo build --release --examples
```

The executables are in `target/release` directory.

### Send HTTP request

A tool for triggering HTTP requests using the http_requester. To run locally,
one must have Kafka, Zookeeper and http\_requester running.

```bash
docker-compose up --build
cargo run --bin http_requester
```

```bash
./target/release/examples/send_http_request --help

HTTP Request Sender 4.20
Censhare Techlab
Sends HTTP requests through Kafka and http_requester

USAGE:
    send_http_request <URI> [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -d, --data <DATA>                            HTTP POST data
    -H, --header <HeaderName: HeaderValue>...
    -s, --kafka_server <SERVER:PORT>             Kafka server to connect to [default: localhost:9092]
    -t, --kafka_topic <TOPIC>                    Kafka topic to write to [default: test.http]
    -X, --request <VERB>                         HTTP verb to use with the request [default: GET]
        --timeout <MILLIS>                       Maximum time allowed to wait for response [default: 2000]

ARGS:
    <URI>    The uri to connect
```

An example run:

```bash
./target/release/examples/send_http_request http://httpbin.org/post -X POST -H "Content-Type: application/json" -H "Foobar: LolOmg" --data '{"foo":"bar"}'
```

The output from the http_requester should be something like:

```bash
  Aug 08 16:20:00.000 INFO Successful HTTP request, event_source: test_script, request_type: POST, request_body: {"foo":"bar"}, status_code: 200, curl: curl -X POST --data "{\"foo\":\"bar\"}" -H "Content-Type: application/json" -H "Foobar: LolOmg" http://httpbin.org/post
```

The response event is in the defined output topic in Kafka.

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
`[kafka]` | `config_topic`  | Application configuration topic            | `"production.applications"`
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
