use common::{
    events::{
        google_notification::FcmResult,
        google_notification::FcmResult_Status,
        google_notification::FcmResult_Status::*,
        push_notification::PushNotification
    },
    kafka::{DeliveryFuture, ResponseProducer},
    metrics::CALLBACKS_COUNTER
};

use fcm::response::{FcmError, FcmResponse};
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
        mut event: PushNotification
    ) -> DeliveryFuture
    {
        error!(
            "No FCM key set for application";
            &event,
            "successful" => false,
        );

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut fcm_result = FcmResult::new();

        fcm_result.set_successful(false);
        fcm_result.set_status(MissingCertificate);

        event.mut_google().set_response(fcm_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_error(
        &self,
        key: Option<Vec<u8>>,
        mut event: PushNotification,
        error: FcmError
    ) -> DeliveryFuture
    {
        error!(
            "Error sending a push notification";
            &event,
            "reason" => format!("{:?}", error)
        );

        let mut fcm_result = FcmResult::new();
        fcm_result.set_successful(false);

        match error {
            FcmError::ServerError(_) => {
                fcm_result.set_status(ServerError);
                CALLBACKS_COUNTER.with_label_values(&["server_error"]).inc();
            }
            FcmError::Unauthorized => {
                fcm_result.set_status(Unauthorized);
                CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();
            }
            FcmError::InvalidMessage(error) => {
                fcm_result.set_status(InvalidMessage);
                CALLBACKS_COUNTER
                    .with_label_values(&["invalid_message"])
                    .inc();
                fcm_result.set_error(error);
            }
        };

        event.mut_google().set_response(fcm_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_response(
        &self,
        key: Option<Vec<u8>>,
        mut event: PushNotification,
        response: &FcmResponse,
    ) -> DeliveryFuture {
        let mut fcm_result = FcmResult::new();

        if let Some(multicast_id) = response.multicast_id {
            fcm_result.set_multicast_id(multicast_id);
        }

        if let Some(canonical_ids) = response.canonical_ids {
            fcm_result.set_canonical_ids(canonical_ids);
        }

        match response.results.as_ref().and_then(|ref results| results.first()) {
            Some(result) => {
                if let Some(ref message_id) = result.message_id {
                    fcm_result.set_message_id(message_id.clone());
                }

                if let Some(ref registration_id) = result.registration_id {
                    fcm_result.set_registration_id(registration_id.clone());
                }

                match result.error {
                    None => {
                        info!(
                            "Successfully sent a push notification";
                            &event,
                            "successful" => true
                        );

                        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    }
                    Some(ref error) => {
                        error!(
                            "Error sending a push notification";
                            &event,
                            "successful" => false,
                            "reason" => error
                        );

                        fcm_result.set_successful(false);

                        let (status, status_str) = Self::error_to_parts(error);
                        CALLBACKS_COUNTER.with_label_values(&[status_str]).inc();

                        fcm_result.set_status(status);
                    }
                }
            }
            None => {
                error!(
                    "Error sending a push notification";
                    &event,
                    "successful" => false,
                    "reason" => "unknown_error"
                );

                CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                fcm_result.set_successful(false);
                fcm_result.set_status(Unknown);
            }
        }

        event.mut_google().set_response(fcm_result);

        self.producer.publish(key, &event)
    }

    fn error_to_parts(error: &str) -> (FcmResult_Status, &'static str) {
        match error {
            "InvalidTtl" =>
                (InvalidTtl, "invalid_ttl"),
            "Unavailable" =>
                (Unavailable, "unavailable"),
            "MessageTooBig" =>
                (MessageTooBig, "message_too_big"),
            "NotRegistered" =>
                (NotRegistered, "not_registered"),
            "InvalidDataKey" =>
               (InvalidDataKey, "invalid_data_key"),
            "MismatchSenderId" =>
                (MismatchSenderId, "mismatch_sender_id"),
            "InvalidPackageName" => {
                (InvalidPackageName, "invalid_package_name")
            }
            "MissingRegistration" => {
                (MissingRegistration, "missing_registration")
            }
            "InvalidRegistration" => {
                (InvalidRegistration, "invalid_registration")
            }
            "DeviceMessageRateExceeded" => {
                (DeviceMessageRateExceeded, "device_message_rate_exceeded")
            }
            "TopicsMessageRateExceeded" => {
                (TopicsMessageRateExceeded, "topics_message_rate_exceeded")
            }
            _ =>
                (Unknown, "unknown_error"),
        }
    }
}

impl Clone for FcmProducer {
    fn clone(&self) -> Self {
        FcmProducer {
            producer: self.producer.clone(),
        }
    }
}
