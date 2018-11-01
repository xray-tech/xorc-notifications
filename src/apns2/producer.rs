use a2::{
    Error,
    Response,
    ErrorReason::*,
};

use common::{
    events::{
        push_result::{
            PushResult,
            PushResult_ResponseAction as ResponseAction
        },
        push_notification::PushNotification,
    },
    kafka::{
        DeliveryFuture,
        ResponseProducer
    },
    metrics::*
};

use heck::SnakeCase;
use crate::CONFIG;

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
        event: PushNotification
    ) -> DeliveryFuture
    {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        info!(
            "Successfully sent a push notification";
            &event,
            "successful" => true
        );

        let result: PushResult = (event, ResponseAction::None).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_err(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
        response: Response
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

        let response_action =
            if let Some(ref reason) = response.error.map(|e| e.reason) {
                let error_label = format!("{:?}", reason).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&error_label]).inc();

                match reason {
                    Unregistered | DeviceTokenNotForTopic | BadDeviceToken =>
                        ResponseAction::UnsubscribeEntity,
                    InternalServerError | Shutdown | ServiceUnavailable | ExpiredProviderToken | Forbidden =>
                        ResponseAction::Retry,
                    _ =>
                        ResponseAction::None,
                }
            } else {
                CALLBACKS_COUNTER.with_label_values(&["Unknown"]).inc();
                ResponseAction::None
            };

        let result: PushResult = (event, response_action).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_fatal(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
        error: Error
    ) -> DeliveryFuture
    {
        let status_label = format!("{:?}", error).to_snake_case();
        let result: PushResult = (event, ResponseAction::Retry).into();

        CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
        self.producer.publish(key, &result)
    }
}

impl Clone for ApnsProducer {
    fn clone(&self) -> Self {
        ApnsProducer {
            producer: self.producer.clone(),
        }
    }
}
