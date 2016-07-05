Apple Push Notification Consumer
================================

Reads events from RabbitMQ and sends push notifications through APNS2.

Usage
-----

After cloning the DMP repository and initializing the git submodules, build the
service by executing `cargo build`. Be sure to have the latest stable Rust and
Cargo installed before continuing.

After compiling the binary, run the service:

```
./target/debug/apns2 -n <NO_OF_THREADS> -c <CONFIG_LOCATION> -s
```

If you leave the `-s` parameter out, it will connect to the production service.

Deployment
----------

Deployment should be done with Jenkins. An alternative is to use the provided
makefile.

```
auto_update                    Update the running Mesos configuration, don't ask questions
update                         Update the running Mesos configuration
upload                         Upload the binary to the repository
```
