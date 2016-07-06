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

APNS P12 conversion
-------------------

To be able to send notifications to a specific application, we need to store
its private key and certificate to the artifactory service. The keys are stored
in `http://artifactory.service.consul:8081/artifactory/apns_keys/<STORE
ID>/push_cert.[pem|key]`. To convert a P12 packaged Apple key to a separate key
and certificate, install openssl to get the required files:

```
openssl pkcs12 -in package.p12 -nodes -out push_cert.key -nocerts
openssl pkcs12 -in package.p12 -out push_cert.pem
```
