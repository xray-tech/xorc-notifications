use futures::{
    Future,
    future::ok,
};

use std::{
    collections::HashMap,
};

use common::{
    events::{
        apple_config::*,
        push_notification::PushNotification,
    },
    logger::{
        LogAction
    },
    metrics::*,
    kafka::EventHandler,
};

use gelf::{
    Message as GelfMessage,
    Error as GelfError
};

use a2::{
    error::Error,
    client::Endpoint,
};

use protobuf::{parse_from_bytes};
use notifier::Notifier;
use producer::ApnsProducer;

use ::GLOG;

pub struct ApnsHandler {
    producer: ApnsProducer,
    notifiers: HashMap<String, Notifier>,
}

impl ApnsHandler {
    pub fn new() -> ApnsHandler {
        let notifiers = HashMap::new();
        let producer = ApnsProducer::new();

        ApnsHandler {
            producer,
            notifiers,
        }
    }

    fn log_config_change(
        &self,
        title: &str,
        event: &AppleConfig,
    ) -> Result<(), GelfError>
    {
        let mut test_msg = GelfMessage::new(String::from(title));

        test_msg.set_metadata("app_id", format!("{}", event.get_application_id()))?;

        if event.has_token() {
            test_msg.set_metadata("endpoint", format!("{:?}", event.get_endpoint()))?;
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
            test_msg.set_metadata("connection_type", "token".to_string())?;

            let token = event.get_token();

            test_msg.set_metadata("key_id", token.get_key_id().to_string())?;
            test_msg.set_metadata("team_id", token.get_team_id().to_string())?;
        } else if event.has_certificate() {
            test_msg.set_metadata("endpoint", format!("{:?}", event.get_endpoint()))?;
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate))?;
            test_msg.set_metadata("connection_type", "certificate".to_string())?;
        } else {
            test_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerDelete))?;
        }

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl EventHandler for ApnsHandler {
    fn handle_notification(
        &self,
        event: PushNotification,
    ) -> Box<Future<Item=(), Error=()> + 'static + Send>
    {
        let producer = self.producer.clone();
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

        CALLBACKS_INFLIGHT.inc();

        if let Some(notifier) = self.notifiers.get(event.get_application_id()) {
            let notification_send = notifier
                .notify(&event)
                .then(move |result| {
                    timer.observe_duration();
                    CALLBACKS_INFLIGHT.dec();

                    match result {
                        Ok(response) =>
                            producer.handle_ok(event, response),
                        Err(Error::ResponseError(e)) =>
                            producer.handle_err(event, e),
                        Err(e) =>
                            producer.handle_fatal(event, e),
                    }
                })
                .then(|_| ok(()));

            Box::new(notification_send)
        } else {
            let connection_error = producer
                .handle_fatal(event, Error::ConnectionError)
                .then(|_| ok(()));

            Box::new(connection_error)
        }
    }

    fn handle_config(&mut self, payload: &[u8]) {
        if let Ok(event) = parse_from_bytes::<AppleConfig>(payload) {
            let _ = self.log_config_change("Push config update", &event);
            let application_id = String::from(event.get_application_id());

            let endpoint = match event.get_endpoint() {
                ConnectionEndpoint::Production => Endpoint::Production,
                ConnectionEndpoint::Sandbox => Endpoint::Sandbox,
            };

            if event.has_token() {
                let token = event.get_token();
                let mut pkcs8 = token.get_pkcs8().clone();

                let notifier_result = Notifier::token(
                    &mut pkcs8,
                    token.get_key_id(),
                    token.get_team_id(),
                    endpoint,
                    event.get_apns_topic(),
                );

                match notifier_result {
                    Ok(notifier) => {
                        self.notifiers.insert(application_id, notifier);
                    },
                    Err(error) => {
                        error!(
                            "Error creating a notifier for application #{}: {:?}",
                            application_id,
                            error
                        );
                    }
                }
            } else if event.has_certificate() {
                let certificate = event.get_certificate();
                let mut pkcs12 = certificate.get_pkcs12().clone();

                let notifier_result = Notifier::certificate(
                    &mut pkcs12,
                    certificate.get_password(),
                    endpoint,
                    event.get_apns_topic(),
                );

                match notifier_result {
                    Ok(notifier) => {
                        self.notifiers.insert(application_id, notifier);
                    },
                    Err(error) => {
                        error!(
                            "Error creating a notifier for application #{}: {:?}",
                            application_id,
                            error
                        );
                    }
                }
            } else {
                if let Some(_) = self.notifiers.remove(&application_id) {
                    info!("Deleted notifier for application #{}", application_id);
                }
            }
        } else {
            error!("Error parsing protobuf");
        }
    }
}
