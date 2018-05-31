use rdkafka::{
    config::ClientConfig,
    producer::{FutureProducer, DeliveryFuture},
};

use common::{
    events::{
        push_notification::PushNotification,
        notification_result::{NotificationResult, NotificationResult_Error},
        webpush_notification::WebPushResult,
        header::Header,
    },
    metrics::{
        CALLBACKS_COUNTER,
    },
};

use gelf::{
    Message as GelfMessage,
    Error as GelfError,
    Level as GelfLevel
};

use ::{
    GLOG,
    CONFIG,
};

use web_push::*;
use chrono::Utc;
use protobuf::Message;
use hyper::Uri;

pub struct ResponseProducer {
    producer: FutureProducer,
}

impl ResponseProducer {
    pub fn new() -> ResponseProducer {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &CONFIG.kafka.brokers)
            .set("produce.offset.report", "true")
            .create()
            .expect("Producer creation error");

        ResponseProducer {
            producer,
        }
    }

    pub fn handle_ok(
        &self,
        mut event: PushNotification
    ) -> DeliveryFuture
    {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let _ = self.log_push_result("Successfully sent a push notification", &event, None);
        let mut web_result = WebPushResult::new();

        web_result.set_successful(true);
        event.mut_web().set_response(web_result);

        self.publish(event, "no_retry")
    }

    pub fn handle_no_cert(
        &self,
        mut event: PushNotification,
    ) -> DeliveryFuture
    {
        error!("Certificate missing for event: '{:?}'", event);
        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        event.mut_web().set_response(web_result);

        self.publish(event, "no_retry")
    }

    pub fn handle_error(
        &self,
        mut event: PushNotification,
        error: WebPushError,
    ) -> DeliveryFuture
    {
        let _ = self.log_push_result("Error sending a push notification", &event, Some(&error));

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        web_result.set_error((&error).into());

        event.mut_web().set_response(web_result);

        CALLBACKS_COUNTER.with_label_values(&[error.short_description()]).inc();

        match error {
            WebPushError::ServerError(retry_after) => {
                match retry_after {
                    Some(duration) =>
                        event.set_retry_after(duration.as_secs() as u32),
                    None => {
                        let duration = Self::calculate_retry_duration(&event);
                        event.set_retry_after(duration)
                    }
                }

                self.publish(event, "retry")
            },
            WebPushError::TimeoutError => {
                let duration = Self::calculate_retry_duration(&event);

                event.set_retry_after(duration);

                self.publish(event, "retry")
            },
            _ => {
                self.publish(event, "no_retry")
            },
        }
    }

    fn publish(
        &self,
        event: PushNotification,
        routing_key: &str,
    ) -> DeliveryFuture
    {
        let response_routing_key = event.get_response_recipient_id();
        let response = event.get_web().get_response();
        let mut header = Header::new();

        header.set_created_at(Utc::now().timestamp_millis());
        header.set_source(String::from("webpush"));
        header.set_recipient_id(String::from(response_routing_key));
        header.set_field_type(String::from("notification.NotificationResult"));

        let mut result_event = NotificationResult::new();
        result_event.set_header(header);
        result_event.set_universe(String::from(event.get_universe()));
        result_event.set_correlation_id(String::from(event.get_correlation_id()));

        if response.has_error() {
            result_event.set_successful(false);
            result_event.set_error((&response.get_error()).into());

            if let NotificationResult_Error::Unsubscribed = result_event.get_error() {
                result_event.set_delete_user(true);
            } else {
                result_event.set_delete_user(false);
            }

            result_event.set_reason(format!("{:?}", response.get_error()));
        } else {
            result_event.set_delete_user(false);
            result_event.set_successful(true);
        }

        self.producer.send_copy::<Vec<u8>, ()>(
            routing_key,
            None,
            Some(&result_event.write_to_bytes().unwrap()),
            None,
            None,
            1000
        )
    }

    fn calculate_retry_duration(event: &PushNotification) -> u32 {
        if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        }
    }

    fn log_push_result(&self, title: &str, event: &PushNotification, error: Option<&WebPushError>) -> Result<(), GelfError> {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_full_message(format!("{:?}", event)).
            set_level(GelfLevel::Informational).
            set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?.
            set_metadata("device_token",   format!("{}", event.get_device_token()))?.
            set_metadata("app_id",         format!("{}", event.get_application_id()))?.
            set_metadata("campaign_id",    format!("{}", event.get_campaign_id()))?.
            set_metadata("event_source",   String::from(event.get_header().get_source()))?;

        if let Ok(uri) = event.get_device_token().parse::<Uri>() {
            if let Some(host) = uri.host() {
                test_msg.set_metadata("push_service", String::from(host))?;
            };
        };

        match error {
            Some(&WebPushError::BadRequest(Some(ref error_info))) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", String::from("BadRequest"))?;
                test_msg.set_metadata("long_error", format!("{}", error_info))?;
            }
            Some(error_msg) => {
                test_msg.set_metadata("successful", String::from("false"))?;
                test_msg.set_metadata("error", format!("{:?}", error_msg))?;
            },
            _ => {
                test_msg.set_metadata("successful", String::from("true"))?;
            }
        }

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl Clone for ResponseProducer {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
        }
    }
}
