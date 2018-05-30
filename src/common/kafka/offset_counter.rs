use rdkafka::{
    message::BorrowedMessage,
    consumer::{
        stream_consumer::StreamConsumer,
        Consumer
    },
};

use std::{
    time::SystemTime,
    io,
};

pub struct OffsetCounter<'a> {
    consumer: &'a StreamConsumer,
    counter: i32,
    time: SystemTime,
}

impl<'a> OffsetCounter<'a> {
    /// A helper to store Kafka offsets.
    pub fn new(consumer: &'a StreamConsumer) -> OffsetCounter<'a> {
        let counter = 0;
        let time = SystemTime::now();

        OffsetCounter {
            consumer,
            counter,
            time,
        }
    }

    /// Stores offset every 10 seconds or 500 events.
    pub fn try_store_offset(&mut self, msg: &BorrowedMessage) -> Result<(), io::Error> {
        let should_commit = match self.time.elapsed() {
            Ok(elapsed) if elapsed.as_secs() > 10 || self.counter >= 500 =>
                true,
            Err(_) => {
                true
            },
            _ => false,
        };

        if should_commit {
            self.store_offset(msg)
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No time to save the offset"))
        }
    }

    /// Stores offset immediately.
    pub fn store_offset(&mut self, msg: &BorrowedMessage) -> Result<(), io::Error> {
        match self.consumer.store_offset(msg) {
            Err(kafka_error) => {
                error!(
                    "Couldn't store offset: {:?}",
                    kafka_error,
                );
                Err(io::Error::new(io::ErrorKind::Other, "Couldn't save offset"))
            },
            Ok(_) => {
                self.counter = 0;
                self.time = SystemTime::now();

                Ok(())
            }
        }
    }
}

