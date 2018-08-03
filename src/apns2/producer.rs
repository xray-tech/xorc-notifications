use a2::{
    error::Error,
    response::Response
};

use common::{
    events::{
        apple_notification::*,
        push_notification::PushNotification
    },
    kafka::{
        DeliveryFuture,
        ResponseProducer
    },
    metrics::*
};

use heck::SnakeCase;
use CONFIG;

pub struct ApnsProducer {
    producer: ResponseProducer,
}

impl ApnsProducer {
    pub fn new() -> ApnsProducer {
        ApnsProducer {
            producer: ResponseProducer::new(&CONFIG.kafka),
        }
    }

    pub fn handle_ok(
        &self,
        key: Option<Vec<u8>>,
        mut event: PushNotification
    ) -> DeliveryFuture
    {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        info!(
            "Successfully sent a push notification";
            &event,
            "successful" => true
        );

        let mut apns_result = ApnsResult::new();

        apns_result.set_successful(true);
        apns_result.set_status(ApnsResult_Status::Success);
        event.mut_apple().set_result(apns_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_err(
        &self,
        key: Option<Vec<u8>>,
        mut event: PushNotification,
        response: &Response
    ) -> DeliveryFuture
    {
        let reason = response.error.as_ref()
            .map(|ref error| {
                format!("{:?}", error.reason)
            });

        error!(
            "Error sending a push notification";
            &event,
            "successful" => false,
            "reason" => reason
        );

        let mut apns_result = ApnsResult::new();
        let status: ApnsResult_Status = response.code.into();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        if let Some(ref error) = response.error {
            apns_result.set_reason((&error.reason).into());

            if let Some(ts) = error.timestamp {
                apns_result.set_timestamp(ts as i64);
            }
        };

        match response.error {
            Some(ref reason) => {
                let reason_label = format!("{:?}", reason.reason).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&reason_label]).inc();
            }
            None => {
                let status_label = format!("{:?}", status).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
            }
        }

        event.mut_apple().set_result(apns_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_fatal(
        &self,
        key: Option<Vec<u8>>,
        mut event: PushNotification,
        error: &Error
    ) -> DeliveryFuture
    {
        let mut apns_result = ApnsResult::new();

        let status = match error {
            Error::TimeoutError    => ApnsResult_Status::Timeout,
            Error::ConnectionError => ApnsResult_Status::MissingChannel,
            _                      => ApnsResult_Status::Unknown,
        };

        let status_label = format!("{:?}", status).to_snake_case();
        CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        event.mut_apple().set_result(apns_result);

        self.producer.publish(key, &event)
    }
}

impl Clone for ApnsProducer {
    fn clone(&self) -> Self {
        ApnsProducer {
            producer: self.producer.clone(),
        }
    }
}
