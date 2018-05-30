use std::{
    collections::HashMap,
};

use common::{
    metrics::*,
    events::push_notification::PushNotification,
    events::google_config::GoogleConfig,
    kafka::EventHandler,
};

use futures::{
    Future,
    future::ok,
};

use notifier::Notifier;
use protobuf::parse_from_bytes;
use producer::FcmProducer;
use gelf;

pub struct FcmHandler {
    producer: FcmProducer,
    api_keys: HashMap<String, String>,
    notifier: Notifier,
}

use ::{GLOG};

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

    fn log_config_change(
        &self,
        title: &str,
        event: &GoogleConfig,
    ) -> Result<(), gelf::Error>
    {
        let mut test_msg = gelf::Message::new(String::from(title));

        test_msg.set_metadata("app_id", format!("{}", event.get_application_id()))?;
        test_msg.set_metadata("api_key", format!("{}", event.get_api_key()))?;

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl EventHandler for FcmHandler {
    fn handle_notification(
        &self,
        event: PushNotification,
    ) -> Box<Future<Item=(), Error=()> + 'static + Send>
    {
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

        CALLBACKS_INFLIGHT.inc();

        if let Some(api_key) = self.api_keys.get(event.get_application_id()) {
            let producer = self.producer.clone();

            let notification_send = self.notifier
                .notify(&event, api_key)
                .then(move |result| {
                    timer.observe_duration();
                    CALLBACKS_INFLIGHT.dec();

                    match result {
                        Ok(response) =>
                            producer.handle_response(
                                event,
                                response
                            ),
                        Err(error) =>
                            producer.handle_error(
                                event,
                                error
                            ),
                    }
                })
                .then(|_| ok(()));

            Box::new(notification_send)
        } else {
            let no_cert = self.producer
                .handle_no_cert(event)
                .then(|_| ok(()));

            Box::new(no_cert)
        }
    }


    fn handle_config(&mut self, payload: &[u8]) {
        if let Ok(event) = parse_from_bytes::<GoogleConfig>(payload) {
            let _ = self.log_config_change("Push config update", &event);

            self.api_keys.insert(
                String::from(event.get_application_id()),
                String::from(event.get_api_key()),
            );
        } else {
            error!("Error parsing protobuf");
        }
    }

}
