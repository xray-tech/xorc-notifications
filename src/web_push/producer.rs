use common::{
    events::{
        push_notification::PushNotification,
        push_result::{
            PushResult,
            PushResult_ResponseAction as ResponseAction
        },
    },
    kafka::{DeliveryFuture, ResponseProducer},
    metrics::CALLBACKS_COUNTER
};

use crate::CONFIG;

use web_push::{*, WebPushError::*};

pub struct WebPushProducer {
    producer: ResponseProducer,
}

impl WebPushProducer {
    pub fn new() -> WebPushProducer {
        WebPushProducer {
            producer: ResponseProducer::new(&CONFIG.kafka),
        }
    }

    pub fn handle_ok(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification
    ) -> DeliveryFuture
    {
        info!(
            "Successfully sent a push notification";
            &event,
            "successful" => true
        );

        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let result: PushResult = (event, ResponseAction::None).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_no_cert(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification
    ) -> DeliveryFuture
    {
        error!(
            "Application is not configured to send web push messages";
            &event,
            "successful" => false
        );

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let result: PushResult = (event, ResponseAction::Retry).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_error(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
        error: &WebPushError
    ) -> DeliveryFuture
    {
        error!(
            "Error sending a push notification";
            &event,
            "successful" => false,
            "reason" => format!("{:?}", error)
        );

        let response_action =
            match error {
                ServerError(_) => ResponseAction::Retry,
                //TimeoutError => ResponseAction::Retry, //commented this out bc this seems to be a deprecated error.
                EndpointNotFound | EndpointNotValid => ResponseAction::UnsubscribeEntity,
                _                                   => ResponseAction::None,
            };

        CALLBACKS_COUNTER.with_label_values(&[error.short_description()]).inc();

        let result: PushResult = (event, response_action).into();
        self.producer.publish(key, &result)
    }
}

impl Clone for WebPushProducer {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
        }
    }
}
