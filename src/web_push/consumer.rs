use std::{
    collections::HashMap,
};

use common::{
    metrics::*,
    events::{
        push_notification::PushNotification,
        application::Application,
    },
    kafka::EventHandler,
};

use futures::{
    Future,
    future::ok,
};

use notifier::Notifier;
use producer::WebPushProducer;
use ::{GLOG};

struct ApiKey {
    fcm_api_key: Option<String>
}


pub struct WebPushHandler {
    producer: WebPushProducer,
    fcm_api_keys: HashMap<String, ApiKey>,
    notifier: Notifier,
}

impl WebPushHandler {
    pub fn new() -> WebPushHandler {
        let fcm_api_keys = HashMap::new();
        let producer = WebPushProducer::new();
        let notifier = Notifier::new();

        WebPushHandler {
            producer,
            fcm_api_keys,
            notifier,
        }
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

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();
        let _ = GLOG.log_config_change("Push config update", &application);

        if !application.has_web() {
            if let Some(_) = self.fcm_api_keys.remove(application_id) {
                info!("Deleted notifier for application #{}", application_id);
            };

            return
        }

        let web_app = application.get_web();

        if web_app.has_fcm_api_key() {
            self.fcm_api_keys.insert(
                String::from(application_id),
                ApiKey { fcm_api_key: Some(web_app.get_fcm_api_key().to_string())},
            );
        } else {
            self.fcm_api_keys.insert(
                String::from(application_id),
                ApiKey { fcm_api_key: None },
            );
        }
    }
}
