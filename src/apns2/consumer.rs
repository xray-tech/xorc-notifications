use futures::{Future, future::ok};

use std::collections::HashMap;

use common::{
    events::{
        crm::{
            Application,
            IosConfig_ConnectionEndpoint::{
                Production,
                Sandbox
            },
            IosCertificate,
            IosToken
        },
        push_notification::PushNotification,
        http_request::HttpRequest,
    },
    kafka::EventHandler,
    metrics::*
};

use a2::{client::Endpoint, error::Error};

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
        apns_topic: &str
    ) -> Result<(), Error> {
        let mut pkcs12 = certificate.get_pkcs12();

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
        let mut pkcs8 = token.get_pkcs8();

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
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
        let producer = self.producer.clone();
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

        CALLBACKS_INFLIGHT.inc();

        if let Some(notifier) = self.notifiers.get(event.get_universe()) {
            let notification_send = notifier
                .notify(&event)
                .then(move |result| {
                    timer.observe_duration();
                    CALLBACKS_INFLIGHT.dec();

                    match result {
                        Ok(_) => producer.handle_ok(key, event),
                        Err(Error::ResponseError(e)) => producer.handle_err(key, event, &e),
                        Err(e) => producer.handle_fatal(key, event, &e),
                    }
                })
                .then(|_| ok(()));

            Box::new(notification_send)
        } else {
            let connection_error = producer
                .handle_fatal(key, event, &Error::ConnectionError)
                .then(|_| ok(()));

            Box::new(connection_error)
        }
    }

    fn handle_http(
        &self,
        _: Option<Vec<u8>>,
        _: HttpRequest,
    ) -> Box<Future<Item=(), Error=()> + 'static + Send> {
        warn!("We don't handle http request events here");
        Box::new(ok(()))
    }

    fn handle_config(&mut self, application: Application) {
        let application_id = application.get_id();

        info!("Push config update"; &application);

        if !application.has_ios_config() {
            if self.notifiers.remove(application_id).is_some() {
                warn!("Application removed"; &application);
            }

            return;
        }

        let ios_config = application.get_ios_config();

        if !ios_config.get_enabled() {
            if self.notifiers.remove(application_id).is_some() {
                warn!("Application disabled"; &application);
            }

            return;
        }

        let endpoint = match ios_config.get_endpoint() {
            Production => Endpoint::Production,
            Sandbox => Endpoint::Sandbox,
        };

        let result = if ios_config.has_token() {
            let token_config = ios_config.get_token();

            info!(
                "Updating application configuration";
                &application,
                "connection_type" => "token",
                "team_id" => token_config.get_team_id(),
                "key_id" => token_config.get_key_id(),
                "apns_topic" => ios_config.get_apns_topic(),
                "endpoint" => format!("{:?}", endpoint)
            );

            self.add_token_notifier(
                token_config,
                endpoint,
                application_id,
                ios_config.get_apns_topic(),
            )
        } else {
            info!(
                "Updating application configuration";
                &application,
                "connection_type" => "certificate",
                "apns_topic" => ios_config.get_apns_topic(),
                "endpoint" => format!("{:?}", endpoint)
            );

            self.add_certificate_notifier(
                ios_config.get_certificate(),
                endpoint,
                application_id,
                ios_config.get_apns_topic(),
            )
        };

        if let Err(error) = result {
            error!(
                "Error connecting to APNs";
                &application,
                "error" => format!("{:?}", error)
            )
        };
    }
}
