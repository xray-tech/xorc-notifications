use std::collections::HashMap;

use common::{events::{crm::Application, push_notification::PushNotification},
             kafka::EventHandler, metrics::*};

use futures::{Future, future::ok};

use notifier::Notifier;
use producer::FcmProducer;

pub struct FcmHandler {
    producer: FcmProducer,
    api_keys: HashMap<String, String>,
    notifier: Notifier,
}

use GLOG;

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
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
        CALLBACKS_INFLIGHT.inc();

        if let Some(api_key) = self.api_keys.get(event.get_application_id()) {
            let producer = self.producer.clone();

            Box::new(
                self.notifier
                    .notify(&event, api_key)
                    .then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();

                        match result {
                            Ok(response) => producer.handle_response(event, response),
                            Err(error) => producer.handle_error(event, error),
                        }
                    })
                    .then(|_| ok(())),
            )
        } else {
            Box::new(self.producer.handle_no_cert(event).then(|_| ok(())))
        }
    }

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();

        let _ = GLOG.log_config_change("Push config update", &application);

        if !application.has_android_config() {
            if let Some(_) = self.api_keys.remove(application_id) {
                info!("Deleted notifier for application #{}", application_id);
            };

            return;
        }

        let api_key = application
            .get_android_config()
            .get_fcm_config()
            .get_fcm_api_key();

        self.api_keys.insert(
            String::from(application_id),
            String::from(api_key),
        );
    }
}
