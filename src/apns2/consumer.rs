use futures::{Future, future::ok};

use std::collections::HashMap;

use common::{events::{crm::{Application,
                            IosConfig_ConnectionEndpoint::{Production, Sandbox},
                            IosCertificate, IosToken},
                      push_notification::PushNotification},
             kafka::EventHandler, metrics::*};

use a2::{client::Endpoint, error::Error};

use GLOG;
use notifier::Notifier;
use producer::ApnsProducer;

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

    fn add_certificate_notifier(
        &mut self,
        certificate: &IosCertificate,
        endpoint: Endpoint,
        application_id: &str,
        apns_topic: &str,
    ) -> Result<(), Error> {
        let mut pkcs12 = certificate.get_pkcs12().clone();

        let notifier = Notifier::certificate(
            &mut pkcs12,
            certificate.get_password(),
            endpoint,
            apns_topic,
        )?;

        self.notifiers.insert(application_id.to_string(), notifier);

        Ok(())
    }

    fn add_token_notifier(
        &mut self,
        token: &IosToken,
        endpoint: Endpoint,
        application_id: &str,
        apns_topic: &str,
    ) -> Result<(), Error> {
        let mut pkcs8 = token.get_pkcs8().clone();

        let notifier = Notifier::token(
            &mut pkcs8,
            token.get_key_id(),
            token.get_team_id(),
            endpoint,
            apns_topic,
        )?;

        self.notifiers.insert(application_id.to_string(), notifier);

        Ok(())
    }
}

impl EventHandler for ApnsHandler {
    fn handle_notification(
        &self,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
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
                        Ok(response) => producer.handle_ok(event, response),
                        Err(Error::ResponseError(e)) => producer.handle_err(event, e),
                        Err(e) => producer.handle_fatal(event, e),
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

        if !application.has_ios_config() {
            if let Some(_) = self.notifiers.remove(application_id) {
                info!("Deleted notifier for application #{}", application_id);
            };

            return;
        }

        let ios_config = application.get_ios_config();

        let endpoint = match ios_config.get_endpoint() {
            Production => Endpoint::Production,
            Sandbox => Endpoint::Sandbox,
        };

        let result = if ios_config.has_token() {
            self.add_token_notifier(
                ios_config.get_token(),
                endpoint,
                application_id,
                ios_config.get_apns_topic(),
            )
        } else {
            self.add_certificate_notifier(
                ios_config.get_certificate(),
                endpoint,
                application_id,
                ios_config.get_apns_topic(),
            )
        };

        if let Err(error) = result {
            error!(
                "Error creating a notifier for application #{}: {:?}",
                application_id, error
            )
        };
    }
}
