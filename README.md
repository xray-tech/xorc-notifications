Web Push Consumer
=================

Reads events from RabbitMQ and sends web push notifications.

Protobuf support
----------------

To do anything with the code one must install a protobuf compiler `protoc` that
supports the version 2 of Protocol Buffers including support for the Rust
backend.

Installation instructions can be found from
the [rust-protobuf Github page](https://github.com/stepancheg/rust-protobuf).

Usage
-----

After cloning the web-push repository and initializing the git submodules, build the
service by executing `cargo build`. Be sure to have the latest stable Rust and
Cargo installed before continuing.

After compiling the binary, run the service:

```
./target/debug/web-push -n <NO_OF_THREADS> -c <CONFIG_LOCATION>
```

Deployment
----------

Deployment should be done with Jenkins. An alternative is to use the provided
makefile.

```
auto_update                    Update the running Mesos configuration, don't ask questions
update                         Update the running Mesos configuration
upload                         Upload the binary to the repository
```

Customer API keys
-----------------

To be able to send notifications to a specific application, we need to store
its api key to the artifactory. The key is stored
in `http://artifactory.service.consul:8081/artifactory/fcm_keys/<STORE
ID>/api_key`.
