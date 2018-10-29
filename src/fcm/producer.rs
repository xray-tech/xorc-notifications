use common::{
    events::{
        push_result::{
            PushResult,
            PushResult_ResponseAction as ResponseAction
        },
        push_notification::PushNotification,
    },
    kafka::{DeliveryFuture, ResponseProducer},
    metrics::CALLBACKS_COUNTER
};

use fcm::response::{FcmError, FcmResponse, ErrorReason::*};
use CONFIG;

pub struct FcmProducer {
    producer: ResponseProducer,
}

impl FcmProducer {
    pub fn new() -> FcmProducer {
        FcmProducer {
            producer: ResponseProducer::new(&CONFIG.kafka),
        }
    }

    pub fn handle_no_cert(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification
    ) -> DeliveryFuture
    {
        error!(
            "No FCM key set for application";
            &event,
            "successful" => false,
        );

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let result: PushResult = (event, ResponseAction::Retry).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_error(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
        error: FcmError
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
                FcmError::ServerError(_) => {
                    CALLBACKS_COUNTER.with_label_values(&["server_error"]).inc();
                    ResponseAction::Retry
                }
                FcmError::Unauthorized => {
                    CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();
                    ResponseAction::None
                }
                FcmError::InvalidMessage(_) => {
                    CALLBACKS_COUNTER.with_label_values(&["invalid_message"]).inc();
                    ResponseAction::None
                }
            };

        let result: PushResult = (event, response_action).into();
        self.producer.publish(key, &result)
    }

    pub fn handle_response(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
        response: FcmResponse,
    ) -> DeliveryFuture {
        let error = response
            .results
            .as_ref()
            .and_then(|ref results| results.first())
            .and_then(|ref result| result.error);

        let response_action =
            if let Some(ref error) = error {
                let status_str = format!("{:?}", error);
                CALLBACKS_COUNTER.with_label_values(&[&status_str]).inc();

                error!(
                    "Error sending a push notification";
                    &event,
                    "successful" => false,
                    "reason" => status_str
                );

                match error {
                    NotRegistered => ResponseAction::UnsubscribeEntity,
                    _             => ResponseAction::None,
                }
            } else {
                ResponseAction::None
            };

        let result: PushResult = (event, response_action).into();
        self.producer.publish(key, &result)
    }
}

impl Clone for FcmProducer {
    fn clone(&self) -> Self {
        FcmProducer {
            producer: self.producer.clone(),
        }
    }
}
