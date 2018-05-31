use std::{
    collections::HashMap,
};

use common::{
    metrics::*,
    events::push_notification::PushNotification,
    events::web_push_config::WebPushConfig,
    kafka::EventHandler,
};

use futures::{
    Future,
    future::ok,
};

use notifier::Notifier;
use protobuf::parse_from_bytes;
use producer::ResponseProducer;
use gelf;
use ::{GLOG};

struct ApiKey {
    fcm_api_key: Option<String>
}


pub struct WebPushHandler {
    producer: ResponseProducer,
    fcm_api_keys: HashMap<String, ApiKey>,
    notifier: Notifier,
}

impl WebPushHandler {
    pub fn new() -> WebPushHandler {
        let fcm_api_keys = HashMap::new();
        let producer = ResponseProducer::new();
        let notifier = Notifier::new();

        WebPushHandler {
            producer,
            fcm_api_keys,
            notifier,
        }
    }

    fn log_config_change(
        &self,
        title: &str,
        event: &WebPushConfig,
    ) -> Result<(), gelf::Error>
    {
        let mut test_msg = gelf::Message::new(String::from(title));

        test_msg.set_metadata("app_id", format!("{}", event.get_application_id()))?;
        test_msg.set_metadata("api_key", format!("{}", event.get_fcm_api_key()))?;

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl EventHandler for WebPushHandler {
    fn handle_notification(
        &self,
        event: PushNotification
    ) -> Box<Future<Item=(), Error=()> + 'static + Send>
    {

        let producer = self.producer.clone();

        match self.fcm_api_keys.get(event.get_application_id()) {
            Some(entity) => {
                let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
                CALLBACKS_INFLIGHT.inc();

                let notification_send = self.notifier
                    .notify(&event, entity.fcm_api_key.as_ref())
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();

                        match result {
                            Ok(()) =>
                                producer.handle_ok(event),
                            Err(error) =>
                                producer.handle_error(event, error),
                        }
                    })
                    .then(|_| ok(()));

                Box::new(notification_send)
            },
            None => {
                Box::new(self.producer.handle_no_cert(event).then(|_| ok(())))
            }
        }
    }

    fn handle_config(&mut self, payload: &[u8]) {
        if let Ok(event) = parse_from_bytes::<WebPushConfig>(payload) {
            let _ = self.log_config_change("Push config update", &event);

            if event.has_fcm_api_key() {
                self.fcm_api_keys.insert(
                    String::from(event.get_application_id()),
                    ApiKey { fcm_api_key: Some(event.get_fcm_api_key().to_string())},
                );
            } else if event.has_no_fcm_api_key() {
                self.fcm_api_keys.insert(
                    String::from(event.get_application_id()),
                    ApiKey { fcm_api_key: None },
                );
            } else {
                self.fcm_api_keys.remove(event.get_application_id());
            }
        } else {
            error!("Error parsing protobuf");
        }
    }
}
