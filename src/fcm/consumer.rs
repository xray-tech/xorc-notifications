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

use futures::{future::Future, future::{ok, err}, FutureExt};

use std::sync::{RwLock, Arc, Mutex};
use std::thread;
use crate::notifier::Notifier;
use crate::producer::FcmProducer;

use super::common::kafka::{FutureMime};

pub struct FcmHandler {
    producer: Arc<FcmProducer>,
    api_keys: RwLock<HashMap<String, String>>,
    notifier: Arc<Notifier>,
}

impl FcmHandler {
    pub fn new() -> FcmHandler {
        let api_keys = RwLock::new(HashMap::new());
        let producer = Arc::new(FcmProducer::new());
        let notifier = Arc::new(Notifier::new());

        FcmHandler {
            producer,
            api_keys,
            notifier,
        }
    }

    fn delete_key(&self, id: &str) {
        if self.api_keys.write().unwrap().remove(id).is_some() {
            self.set_app_counter();
            info!("Application removed"; "universe" => id);
        };
    }

    fn set_app_counter(&self) {
        NUMBER_OF_APPLICATIONS.set(self.api_keys.read().unwrap().len() as f64);
    }
}

impl EventHandler for FcmHandler {
    fn accepts(&self, event: &PushNotification) -> bool {
        event.has_google()
    }

    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send + Unpin> {
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
        CALLBACKS_INFLIGHT.inc();

        if let Some(api_key) = self.api_keys.read().unwrap().get(event.get_universe()) {
            let producer = self.producer.clone();
            let noti = self.notifier.clone();
            let mute = Arc::new(Mutex::<Option<Result<(),()>>>::new(None));
            let res = mute.clone();
            let key = api_key.clone();
            let even = event.clone();
            thread::spawn(|| async move {
                let even = even;
                let a = noti.notify(&even, &key.clone())
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();
                        match result {
                            Ok(response) => producer.handle_response(Some(Vec::from(key.as_bytes())), event, response),
                            Err(error) => producer.handle_error(Some(Vec::from(key.as_bytes())), event, error),
                        }
                    }).then(|_| ok(())).await;
                *mute.lock().unwrap() = Some(a);
            });
            Box::new(FutureMime::new(res))
        } else {
            Box::new(self.producer.handle_no_cert(key, event).then(|_| err(())))
        }
    }

    fn handle_http(
        &self,
        _: Option<Vec<u8>>,
        _: HttpRequest
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send+ Unpin> {
        warn!("We don't handle http request events here");
        Box::new(err(()))
    }

    fn handle_config(&self, id: &str, application: Option<Application>) {
        match application {
            None => {
                self.delete_key(id);
            }
            Some(application) => {
                let application_id = application.get_id();

                if !application.has_android_config() {
                    self.delete_key(application_id);
                    return;
                }

                let android_config = application.get_android_config();

                if !android_config.get_enabled() {
                    self.delete_key(application_id);
                    return;
                }

                if !android_config.has_fcm_api_key() {
                    self.delete_key(application_id);
                    return;
                }

                let api_key = android_config.get_fcm_api_key();

                info!(
                    "Updating application configuration";
                    &application,
                    "fcm_api_key" => api_key
                );

                self.api_keys.write().unwrap().insert(
                    String::from(application_id),
                    String::from(api_key),
                );

                self.set_app_counter();
            }
        }
    }
}
