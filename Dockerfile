FROM rust:latest
MAINTAINER Julius de Bruijn <julius@nauk.io>

WORKDIR /usr/src
ENV USER root
ENV RUST_BACKTRACE 1

RUN apt-get -y update
RUN apt-get -y install libssl-dev protobuf-compiler libffi-dev build-essential python

ENV PROTOC /usr/bin/protoc
ENV PROTOC_INCLUDE /usr/include

RUN mkdir -p /usr/src/xorc-notifications
RUN mkdir -p /etc/xorc-notifications
COPY Cargo.toml Cargo.lock build.rs /usr/src/xorc-notifications/
COPY src /usr/src/xorc-notifications/src
COPY third_party /usr/src/xorc-notifications/third_party
COPY third_party/events /usr/src/xorc-notifications/third_party/events
COPY config /usr/src/xorc-notifications/config

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /usr/src/xorc-notifications
RUN cargo build --release

CMD "echo 'Xorc notifications base image'"
