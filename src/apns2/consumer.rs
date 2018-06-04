use futures::{
    Future,
    future::ok,
};

use std::{
    collections::HashMap,
};

use common::{
    events::{
        application::{
            Application,
            IosApplication_ConnectionEndpoint::*,
        },
        push_notification::PushNotification,
    },
    metrics::*,
    kafka::EventHandler,
};

use a2::{
    error::Error,
    client::Endpoint,
};

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

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();

        let _ = GLOG.log_config_change("Push config update", &application);

        if !application.has_ios() {
            if let Some(_) = self.notifiers.remove(application_id) {
                info!("Deleted notifier for application #{}", application_id);
            };

            return
        }

        let ios_config = application.get_ios();

        let endpoint = match ios_config.get_endpoint() {
            Production => Endpoint::Production,
            Sandbox    => Endpoint::Sandbox,
        };

        if ios_config.has_token() {
            let token = ios_config.get_token();
            let mut pkcs8 = token.get_pkcs8().clone();

            let notifier_result = Notifier::token(
                &mut pkcs8,
                token.get_key_id(),
                token.get_team_id(),
                endpoint,
                ios_config.get_apns_topic(),
            );

            match notifier_result {
                Ok(notifier) => {
                    self.notifiers.insert(
                        application_id.to_string(),
                        notifier
                    );
                },
                Err(error) => {
                    error!(
                        "Error creating a notifier for application #{}: {:?}",
                        application_id,
                        error
                    );
                }
            }
        } else if ios_config.has_certificate() {
            let certificate = ios_config.get_certificate();
            let mut pkcs12 = certificate.get_pkcs12().clone();

            let notifier_result = Notifier::certificate(
                &mut pkcs12,
                certificate.get_password(),
                endpoint,
                ios_config.get_apns_topic(),
            );

            match notifier_result {
                Ok(notifier) => {
                    self.notifiers.insert(
                        application_id.to_string(),
                        notifier
                    );
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
            error!("Unsupported iOS application type");
        }
    }
}
