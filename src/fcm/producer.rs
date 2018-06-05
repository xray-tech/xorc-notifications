use common::{events::{ResponseAction, google_notification::FcmResult,
                      google_notification::FcmResult_Status::*,
                      push_notification::PushNotification},
             kafka::{DeliveryFuture, ResponseProducer}, metrics::CALLBACKS_COUNTER};

use fcm::response::{FcmError, FcmResponse};

use gelf::{Error as GelfError, Level as GelfLevel, Message as GelfMessage};

use {CONFIG, GLOG};

pub struct FcmProducer {
    producer: ResponseProducer,
}

impl FcmProducer {
    pub fn new() -> FcmProducer {
        FcmProducer {
            producer: ResponseProducer::new(&CONFIG.kafka),
        }
    }

    pub fn handle_no_cert(&self, mut event: PushNotification) -> DeliveryFuture {
        let _ = self.log_result(
            "Error sending a push notification",
            &event,
            Some("MissingCertificateOrToken"),
        );

        CALLBACKS_COUNTER
            .with_label_values(&["certificate_missing"])
            .inc();

        let mut fcm_result = FcmResult::new();

        fcm_result.set_successful(false);
        fcm_result.set_status(MissingCertificate);

        event.mut_google().set_response(fcm_result);

        self.producer.publish(event, ResponseAction::Retry)
    }

    pub fn handle_error(&self, mut event: PushNotification, error: FcmError) -> DeliveryFuture {
        let error_str = format!("{:?}", error);
        let _ = self.log_result(
            "Error sending a push notification",
            &event,
            Some(&error_str),
        );

        let mut fcm_result = FcmResult::new();
        fcm_result.set_successful(false);

        let response_action = match error {
            FcmError::ServerError(_) => {
                fcm_result.set_status(ServerError);
                CALLBACKS_COUNTER.with_label_values(&["server_error"]).inc();

                ResponseAction::Retry
            }
            FcmError::Unauthorized => {
                fcm_result.set_status(Unauthorized);
                CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();

                ResponseAction::UnsubscribeEntity
            }
            FcmError::InvalidMessage(error) => {
                fcm_result.set_status(InvalidMessage);
                CALLBACKS_COUNTER
                    .with_label_values(&["invalid_message"])
                    .inc();
                fcm_result.set_error(error);

                ResponseAction::None
            }
        };

        event.mut_google().set_response(fcm_result);

        self.producer.publish(event, response_action)
    }

    pub fn handle_response(
        &self,
        mut event: PushNotification,
        response: FcmResponse,
    ) -> DeliveryFuture {
        let mut fcm_result = FcmResult::new();

        if let Some(multicast_id) = response.multicast_id {
            fcm_result.set_multicast_id(multicast_id);
        }

        if let Some(canonical_ids) = response.canonical_ids {
            fcm_result.set_canonical_ids(canonical_ids);
        }

        match response.results {
            Some(results) => match results.first() {
                Some(result) => {
                    if let Some(ref message_id) = result.message_id {
                        fcm_result.set_message_id(message_id.clone());
                    }

                    if let Some(ref registration_id) = result.registration_id {
                        fcm_result.set_registration_id(registration_id.clone());
                    }

                    if result.error.is_none() {
                        let _ =
                            self.log_result("Successfully sent a push notification", &event, None);

                        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    } else {
                        let _ = self.log_result(
                            "Error sending a push notification",
                            &event,
                            result.error.as_ref().map(AsRef::as_ref),
                        );

                        fcm_result.set_successful(false);

                        let (status, status_str) = match result.error.as_ref().map(AsRef::as_ref) {
                            Some("InvalidTtl") => (InvalidTtl, "invalid_ttl"),
                            Some("Unavailable") => (Unavailable, "unavailable"),
                            Some("MessageTooBig") => (MessageTooBig, "message_too_big"),
                            Some("NotRegistered") => (NotRegistered, "not_registered"),
                            Some("InvalidDataKey") => (InvalidDataKey, "invalid_data_key"),
                            Some("MismatchSenderId") => (MismatchSenderId, "mismatch_sender_id"),
                            Some("InvalidPackageName") => {
                                (InvalidPackageName, "invalid_package_name")
                            }
                            Some("MissingRegistration") => {
                                (MissingRegistration, "missing_registration")
                            }
                            Some("InvalidRegistration") => {
                                (InvalidRegistration, "invalid_registration")
                            }
                            Some("DeviceMessageRateExceeded") => {
                                (DeviceMessageRateExceeded, "device_message_rate_exceeded")
                            }
                            Some("TopicsMessageRateExceeded") => {
                                (TopicsMessageRateExceeded, "topics_message_rate_exceeded")
                            }
                            _ => (Unknown, "unknown_error"),
                        };

                        CALLBACKS_COUNTER.with_label_values(&[status_str]).inc();

                        fcm_result.set_status(status);
                    }
                }
                None => {
                    let _ = self.log_result(
                        "Error sending a push notification",
                        &event,
                        Some("UnknownError"),
                    );

                    CALLBACKS_COUNTER
                        .with_label_values(&["unknown_error"])
                        .inc();
                    fcm_result.set_successful(false);
                    fcm_result.set_status(Unknown);
                }
            },
            None => {
                let _ = self.log_result(
                    "Error sending a push notification",
                    &event,
                    Some("UnknownError"),
                );

                CALLBACKS_COUNTER
                    .with_label_values(&["unknown_error"])
                    .inc();
                fcm_result.set_successful(false);
                fcm_result.set_status(Unknown);
            }
        }

        let response_action = match fcm_result.get_status() {
            NotRegistered => ResponseAction::UnsubscribeEntity,
            _ => ResponseAction::None,
        };

        event.mut_google().set_response(fcm_result);

        self.producer.publish(event, response_action)
    }

    fn log_result(
        &self,
        title: &str,
        event: &PushNotification,
        error: Option<&str>,
    ) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg
            .set_full_message(format!("{:?}", event))
            .set_level(GelfLevel::Informational)
            .set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?
            .set_metadata("device_token", format!("{}", event.get_device_token()))?
            .set_metadata("app_id", format!("{}", event.get_application_id()))?
            .set_metadata("campaign_id", format!("{}", event.get_campaign_id()))?
            .set_metadata(
                "event_source",
                String::from(event.get_header().get_source()),
            )?;

        if let Some(msg) = error {
            test_msg.set_metadata("successful", String::from("false"))?;
            test_msg.set_metadata("error", format!("{}", msg))?;
        } else {
            test_msg.set_metadata("successful", String::from("true"))?;
        }

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl Clone for FcmProducer {
    fn clone(&self) -> Self {
        FcmProducer {
            producer: self.producer.clone(),
        }
    }
}
