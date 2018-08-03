use std::collections::HashMap;

use common::{
    events::{
        crm::Application,
        push_notification::PushNotification,
        http_request::HttpRequest,
    },
    kafka::EventHandler,
    metrics::*
};

use futures::{Future, future::ok};

use notifier::Notifier;
use producer::FcmProducer;

pub struct FcmHandler {
    producer: FcmProducer,
    api_keys: HashMap<String, String>,
    notifier: Notifier,
}

impl FcmHandler {
    pub fn new() -> FcmHandler {
        let api_keys = HashMap::new();
        let producer = FcmProducer::new();
        let notifier = Notifier::new();

        FcmHandler {
            producer,
            api_keys,
            notifier,
        }
    }
}

impl EventHandler for FcmHandler {
    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
        CALLBACKS_INFLIGHT.inc();

        if let Some(api_key) = self.api_keys.get(event.get_universe()) {
            let producer = self.producer.clone();

            Box::new(
                self.notifier
                    .notify(&event, api_key)
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();

                        match result {
                            Ok(response) => producer.handle_response(key, event, &response),
                            Err(error) => producer.handle_error(key, event, error),
                        }
                    })
                    .then(|_| ok(())),
            )
        } else {
            Box::new(self.producer.handle_no_cert(key, event).then(|_| ok(())))
        }
    }

    fn handle_http(
        &self,
        _: Option<Vec<u8>>,
        _: HttpRequest
    ) -> Box<Future<Item=(), Error=()> + 'static + Send> {
        warn!("We don't handle http request events here");
        Box::new(ok(()))
    }

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();

        if !application.has_android_config() {
            if self.api_keys.remove(application_id).is_some() {
                info!("Application removed"; &application);
            };

            return;
        }

        let android_config = application.get_android_config();

        if !android_config.get_enabled() {
            if self.api_keys.remove(application_id).is_some() {
                warn!("Application disabled"; &application);
            };

            return;
        }

        let api_key = android_config
            .get_fcm_config()
            .get_fcm_api_key();

        info!(
            "Updating application configuration";
            &application,
            "fcm_api_key" => &api_key
        );

        self.api_keys.insert(
            String::from(application_id),
            String::from(api_key),
        );
    }
}
