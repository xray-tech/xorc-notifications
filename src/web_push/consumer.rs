use std::collections::HashMap;

use common::{
    events::{
        application::Application,
        push_notification::PushNotification,
        http_request::HttpRequest,
    },
    kafka::EventHandler,
    metrics::*
};

use futures::{Future, future::ok};
use std::sync::RwLock;
use crate::notifier::Notifier;
use crate::producer::WebPushProducer;

struct ApiKey {
    fcm_api_key: Option<String>,
}

pub struct WebPushHandler {
    producer: WebPushProducer,
    fcm_api_keys: RwLock<HashMap<String, ApiKey>>,
    notifier: Notifier,
}

impl WebPushHandler {
    pub fn new() -> WebPushHandler {
        let fcm_api_keys = RwLock::new(HashMap::new());
        let producer = WebPushProducer::new();
        let notifier = Notifier::new();

        WebPushHandler {
            producer,
            fcm_api_keys,
            notifier,
        }
    }

    fn delete_key(&self, id: &str) {
        if self.fcm_api_keys.write().unwrap().remove(id).is_some() {
            self.set_app_counter();
            info!("Application removed"; "universe" => id);
        };
    }

    fn set_app_counter(&self) {
        NUMBER_OF_APPLICATIONS.set(self.fcm_api_keys.read().unwrap().len() as f64);
    }
}

impl EventHandler for WebPushHandler {
    fn accepts(&self, event: &PushNotification) -> bool {
        event.has_web()
    }

    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<dyn Future<Item = (), Error = ()> + 'static + Send> {
        let producer = self.producer.clone();

        match self.fcm_api_keys.read().unwrap().get(event.get_universe()) {
            Some(entity) => {
                let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
                CALLBACKS_INFLIGHT.inc();

                let notification_send = self.notifier
                    .notify(&event, entity.fcm_api_key.as_ref())
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();

                        match result {
                            Ok(()) => producer.handle_ok(key, event),
                            Err(error) => producer.handle_error(key, event, &error),
                        }
                    })
                    .then(|_| ok(()));

                Box::new(notification_send)
            }
            None => Box::new(self.producer.handle_no_cert(key, event).then(|_| ok(()))),
        }
    }

    fn handle_http(
        &self,
        _: Option<Vec<u8>>,
        _: HttpRequest
    ) -> Box<dyn Future<Item=(), Error=()> + 'static + Send> {
        warn!("We don't handle http request events here");
        Box::new(ok(()))
    }

    fn handle_config(&self, id: &str, application: Option<Application>) {
        match application {
            None => {
                self.delete_key(id);
            }
            Some(application) => {
                let application_id = application.get_id();

                if !application.has_web_config() {
                    self.delete_key(application_id);
                    return;
                }

                let web_app = application.get_web_config();

                if !web_app.get_enabled() {
                    self.delete_key(application_id);
                    return;
                }

                if web_app.has_fcm_api_key() {
                    let api_key = web_app
                        .get_fcm_api_key()
                        .to_string();

                    info!(
                        "Updating application configuration";
                        &application,
                        "fcm_api_key" => &api_key
                    );

                    self.fcm_api_keys.write().unwrap().insert(
                        String::from(application_id),
                        ApiKey { fcm_api_key: Some(api_key), },
                    );
                } else {
                    info!(
                        "Updating application configuration";
                        &application,
                    );

                    self.fcm_api_keys
                        .write()
                        .unwrap()
                        .insert(String::from(application_id), ApiKey { fcm_api_key: None });
                }

                self.set_app_counter();
            }
        }
    }
}
