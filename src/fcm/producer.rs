use rdkafka::{
    config::ClientConfig,
    producer::{FutureProducer, DeliveryFuture},
};

use common::{
    events::{
        push_notification::PushNotification,
        notification_result::{NotificationResult, NotificationResult_Error},
        google_notification::FcmResult,
        google_notification::FcmResult_Status::*,
        header::Header,
    },
    metrics::{
        CALLBACKS_COUNTER,
    },
};


use fcm::{
    response::{
        FcmError,
        FcmResponse,
    }
};

use gelf::{
    Message as GelfMessage,
    Error as GelfError,
    Level as GelfLevel
};

use ::{GLOG, CONFIG};

use chrono::Utc;
use protobuf::Message;

pub struct FcmProducer {
    producer: FutureProducer,
}

impl FcmProducer {
    pub fn new() -> FcmProducer {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &CONFIG.kafka.brokers)
            .set("produce.offset.report", "true")
            .create()
            .expect("Producer creation error");

        FcmProducer {
            producer,
        }
    }

    fn get_retry_after(event: &PushNotification) -> u32 {
        if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        }
    }

    pub fn handle_no_cert(
        &self,
        mut event: PushNotification
    ) -> DeliveryFuture
    {
        let _ = self.log_result(
            "Error sending a push notification",
            &event,
            Some("MissingCertificateOrToken"));

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut fcm_result = FcmResult::new();

        fcm_result.set_successful(false);
        fcm_result.set_status(MissingCertificate);

        event.mut_google().set_response(fcm_result);

        self.publish(event, "no_retry")
    }

    pub fn handle_error(
        &self,
        mut event: PushNotification,
        error: FcmError
    ) -> DeliveryFuture
    {
        let error_str = format!("{:?}", error);
        let _ = self.log_result("Error sending a push notification", &event, Some(&error_str));

        let mut fcm_result = FcmResult::new();
        fcm_result.set_successful(false);

        let routing_key = match error {
            FcmError::ServerError(_) => {
                fcm_result.set_status(ServerError);
                CALLBACKS_COUNTER.with_label_values(&["server_error"]).inc();

                let retry_after = Self::get_retry_after(&event);
                event.set_retry_after(retry_after);

                "retry"
            },
            FcmError::Unauthorized             => {
                fcm_result.set_status(Unauthorized);
                CALLBACKS_COUNTER.with_label_values(&["unauthorized"]).inc();
                "no_retry"
            },
            FcmError::InvalidMessage(error)    => {
                fcm_result.set_status(InvalidMessage);
                CALLBACKS_COUNTER.with_label_values(&["invalid_message"]).inc();
                fcm_result.set_error(error);
                "no_retry"
            },
        };

        event.mut_google().set_response(fcm_result);

        self.publish(event, routing_key)
    }

    pub fn handle_response(
        &self,
        mut event: PushNotification,
        response: FcmResponse
    ) -> DeliveryFuture
    {
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
                        let _ = self.log_result("Successfully sent a push notification", &event, None);

                        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();
                        fcm_result.set_successful(true);
                        fcm_result.set_status(Success);
                    } else {
                        let _ = self.log_result("Error sending a push notification", &event, result.error.as_ref().map(AsRef::as_ref));

                        fcm_result.set_successful(false);

                        let (status, status_str) = match result.error.as_ref().map(AsRef::as_ref) {
                            Some("InvalidTtl")                => (InvalidTtl, "invalid_ttl"),
                            Some("Unavailable")               => (Unavailable, "unavailable"),
                            Some("MessageTooBig")             => (MessageTooBig, "message_too_big"),
                            Some("NotRegistered")             => (NotRegistered, "not_registered"),
                            Some("InvalidDataKey")            => (InvalidDataKey, "invalid_data_key"),
                            Some("MismatchSenderId")          => (MismatchSenderId, "mismatch_sender_id"),
                            Some("InvalidPackageName")        => (InvalidPackageName, "invalid_package_name"),
                            Some("MissingRegistration")       => (MissingRegistration, "missing_registration"),
                            Some("InvalidRegistration")       => (InvalidRegistration, "invalid_registration"),
                            Some("DeviceMessageRateExceeded") => (DeviceMessageRateExceeded, "device_message_rate_exceeded"),
                            Some("TopicsMessageRateExceeded") => (TopicsMessageRateExceeded, "topics_message_rate_exceeded"),
                            _                                 => (Unknown, "unknown_error"),
                        };

                        CALLBACKS_COUNTER.with_label_values(&[status_str]).inc();

                        fcm_result.set_status(status);
                    }
                },
                None => {
                    let _ = self.log_result("Error sending a push notification", &event, Some("UnknownError"));

                    CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                    fcm_result.set_successful(false);
                    fcm_result.set_status(Unknown);
                }
            },
            None => {
                let _ = self.log_result("Error sending a push notification", &event, Some("UnknownError"));

                CALLBACKS_COUNTER.with_label_values(&["unknown_error"]).inc();
                fcm_result.set_successful(false);
                fcm_result.set_status(Unknown);
            }
        }

        event.mut_google().set_response(fcm_result);

        self.publish(event, "no_retry")
    }

    fn publish(
        &self,
        event: PushNotification,
        topic: &str
    ) -> DeliveryFuture
    {
        let response             = event.get_google().get_response();
        let mut header           = Header::new();

        header.set_created_at(Utc::now().timestamp_millis());
        header.set_source(String::from("fcm"));
        header.set_recipient_id(String::from("MISSING TODO TODO TODO"));
        header.set_field_type(String::from("notification.NotificationResult"));

        let mut result_event = NotificationResult::new();
        result_event.set_header(header);
        result_event.set_universe(String::from(event.get_universe()));
        result_event.set_correlation_id(String::from(event.get_correlation_id()));

        match response.get_status() {
            Success => {
                result_event.set_delete_user(false);
                result_event.set_successful(true);
            },
            NotRegistered => {
                result_event.set_delete_user(true);
                result_event.set_successful(false);
                result_event.set_error(NotificationResult_Error::Unsubscribed);
            },
            _ => {
                result_event.set_delete_user(false);
                result_event.set_successful(false);
                result_event.set_reason(format!("{:?}", response.get_status()));
                result_event.set_error(NotificationResult_Error::Other);
            },
        }

        self.producer.send_copy::<Vec<u8>, ()>(
            topic,
            None,
            Some(&result_event.write_to_bytes().unwrap()),
            None,
            None,
            1000
        )
    }

    fn log_result(&self, title: &str, event: &PushNotification, error: Option<&str>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

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
