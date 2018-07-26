use std::collections::HashMap;

use common::{events::{crm::Application, push_notification::PushNotification},
             kafka::EventHandler, metrics::*};

use futures::{Future, future::ok};

use notifier::Notifier;
use producer::WebPushProducer;

struct ApiKey {
    fcm_api_key: Option<String>,
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
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
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
                            Ok(()) => producer.handle_ok(event),
                            Err(error) => producer.handle_error(event, &error),
                        }
                    })
                    .then(|_| ok(()));

                Box::new(notification_send)
            }
            None => Box::new(self.producer.handle_no_cert(event).then(|_| ok(()))),
        }
    }

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();

        if !application.has_web_config() {
            if self.fcm_api_keys.remove(application_id).is_some() {
                info!("Application removed"; &application);
            };

            return;
        }

        let web_app = application.get_web_config();

        if !web_app.get_enabled() {
            if self.fcm_api_keys.remove(application_id).is_some() {
                warn!("Application disabled"; &application);
            };

            return;
        }

        if web_app.has_fcm_config() {
            let api_key = web_app
                .get_fcm_config()
                .get_fcm_api_key()
                .to_string();

            info!(
                "Updating application configuration";
                &application,
                "fcm_api_key" => &api_key
            );

            self.fcm_api_keys.insert(
                String::from(application_id),
                ApiKey { fcm_api_key: Some(api_key), },
            );
        } else {
            info!(
                "Updating application configuration";
                &application,
            );

            self.fcm_api_keys
                .insert(String::from(application_id), ApiKey { fcm_api_key: None });
        }
    }
}
