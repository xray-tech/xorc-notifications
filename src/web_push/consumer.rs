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

use futures::{Future, future::{ok,err}};
use futures::FutureExt;
use std::sync::RwLock;
use crate::notifier::Notifier;
use crate::producer::WebPushProducer;

//use super::common::kafka::{Erro,to_async,Rdy};
use core::marker::Unpin;
use web_push::{VapidSignature, WebPushError, SubscriptionInfo, SubscriptionKeys, VapidSignatureBuilder};
use std::fs::File;
use std::env;
use std::sync::Arc;

struct ApiKey {
    fcm_api_key: Option<VapidSignature>,
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
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send + Unpin> {
        let producer = self.producer.clone();

        match self.fcm_api_keys.read().unwrap().get(event.get_universe()) {
            Some(entity) => {
                /*let notification_send = self.notifier.notify(&event, entity.fcm_api_key.as_ref());
                let retu = dahoo(producer,entity, notification_send, key, event);
                Box::new(retu)*/

                let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
                CALLBACKS_INFLIGHT.inc();

                Box::new(self.notifier
                    .notify(&event, entity.fcm_api_key.as_ref())
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();

                        match result {
                            Ok(_) => producer.handle_ok(key, event),
                            Err(x) => {
                                let error = match x {
                                    Ok(x) => x.clone(),
                                    Err(_) => Arc::new(WebPushError::ServerError(None))
                                };
                                producer.handle_error(key, event, &error)
                            }
                            ,
                        }
                    })
                    .then(|_| ok(())))
            }
            None => Box::new(self.producer.handle_no_cert(key, event).then(|_| ok(()))),
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

                    let api = web_app
                        .get_fcm_api_key()
                        .to_string();
                    let api2 = api.clone();

                    let subscription_info = SubscriptionInfo {
                        keys: SubscriptionKeys {
                            p256dh: env::var("WEB_PUSH_P256DH").expect("Panicked trying to get WEB_PUSH_P256DH from env vars! {}"),
                            auth: api,
                        },
                        endpoint: env::var("ENDPOINT").expect("Panicked trying to get ENDPOINT from env vars!"),
                    };

                    let file = File::open("web_push_private.pem").unwrap();

                    let sig_builder = VapidSignatureBuilder::from_pem(file, &subscription_info).unwrap();

                    let api_key = sig_builder.build().unwrap();

                    info!(
                        "Updating application configuration";
                        &application,
                        "fcm_api_key" => api2
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
