use common::{
    events::{
        push_notification::PushNotification,
        webpush_notification::WebPushResult,
        ResponseAction,
    },
    metrics::{
        CALLBACKS_COUNTER,
    },
    kafka::ResponseProducer,
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
use hyper::Uri;
use rdkafka::producer::DeliveryFuture;

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
        mut event: PushNotification
    ) -> DeliveryFuture
    {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let _ = self.log_push_result("Successfully sent a push notification", &event, None);
        let mut web_result = WebPushResult::new();

        web_result.set_successful(true);
        event.mut_web().set_response(web_result);

        self.producer.publish(event, ResponseAction::None)
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

        self.producer.publish(event, ResponseAction::Retry)
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

        let response_action = match error {
            WebPushError::ServerError(_) => {
                ResponseAction::Retry
            },
            WebPushError::TimeoutError => {
                ResponseAction::Retry
            },
            WebPushError::EndpointNotValid | WebPushError::EndpointNotFound => {
                ResponseAction::UnsubscribeEntity
            }
            _ => {
                ResponseAction::None
            },
        };

        self.producer.publish(event, response_action)
    }

    fn log_push_result(
        &self,
        title: &str,
        event: &PushNotification,
        error: Option<&WebPushError>
    ) -> Result<(), GelfError>
    {
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

impl Clone for WebPushProducer {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
        }
    }
}
