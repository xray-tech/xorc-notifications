use common::{
    events::{
        push_notification::PushNotification,
        webpush_notification::WebPushResult
    },
    kafka::{DeliveryFuture, ResponseProducer},
    metrics::CALLBACKS_COUNTER
};

use CONFIG;

use web_push::*;

pub struct WebPushProducer {
    producer: ResponseProducer,
}

impl WebPushProducer {
    pub fn new() -> WebPushProducer {
        WebPushProducer {
            producer: ResponseProducer::new(&CONFIG.kafka),
        }
    }

    pub fn handle_ok(&self, key: Option<Vec<u8>>, mut event: PushNotification) -> DeliveryFuture {
        info!(
            "Successfully sent a push notification";
            &event,
            "successful" => true
        );

        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let mut web_result = WebPushResult::new();

        web_result.set_successful(true);
        event.mut_web().set_response(web_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_no_cert(&self, key: Option<Vec<u8>>, mut event: PushNotification) -> DeliveryFuture {
        error!(
            "Application is not configured to send web push messages";
            &event,
            "successful" => false
        );

        CALLBACKS_COUNTER.with_label_values(&["certificate_missing"]).inc();

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        event.mut_web().set_response(web_result);

        self.producer.publish(key, &event)
    }

    pub fn handle_error(&self, key: Option<Vec<u8>>, mut event: PushNotification, error: &WebPushError) -> DeliveryFuture {
        error!(
            "Error sending a push notification";
            &event,
            "successful" => false,
            "reason" => format!("{:?}", error)
        );

        let mut web_result = WebPushResult::new();

        web_result.set_successful(false);
        web_result.set_error(error.into());

        event.mut_web().set_response(web_result);

        CALLBACKS_COUNTER.with_label_values(&[error.short_description()]).inc();

        self.producer.publish(key, &event)
    }
}

impl Clone for WebPushProducer {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
        }
    }
}
