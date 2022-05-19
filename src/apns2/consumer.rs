use futures::{Future, FutureExt, future::{ok, err}};

use std::{collections::HashMap, sync::{RwLock, Arc, Mutex}, thread};

use common::{
    events::{
        application::{
            Application,
            ConnectionEndpoint::{
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

use crate::notifier::Notifier;
use crate::producer::ApnsProducer;

use super::common::kafka::{FutureMime};

pub struct ApnsHandler {
    producer: Arc<ApnsProducer>,
    notifiers: RwLock<HashMap<String, Arc<Notifier>>>,
}

impl ApnsHandler {
    pub fn new() -> ApnsHandler {
        let notifiers = RwLock::new(HashMap::new());
        let producer = Arc::new(ApnsProducer::new());

        ApnsHandler {
            producer,
            notifiers,
        }
    }

    fn add_certificate_notifier(
        &self,
        certificate: &IosCertificate,
        endpoint: Endpoint,
        application_id: &str,
        apns_topic: &str
    ) -> Result<(), Error> {
        let mut pkcs12 = certificate.get_pkcs12();

        let notifier = Arc::new(Notifier::certificate(
        &mut pkcs12,
            certificate.get_password(),
            endpoint,
            apns_topic,
        )?);

        let mut notifiers = self.notifiers.write().unwrap();
        notifiers.insert(application_id.to_string(), notifier);

        Ok(())
    }

    fn add_token_notifier(
        &self,
        token: &IosToken,
        endpoint: Endpoint,
        application_id: &str,
        apns_topic: &str,
    ) -> Result<(), Error> {
        let mut pkcs8 = token.get_pkcs8();

        let notifier = Arc::new(Notifier::token(
            &mut pkcs8,
            token.get_key_id(),
            token.get_team_id(),
            endpoint,
            apns_topic,
        )?);

        let mut notifiers = self.notifiers.write().unwrap();
        notifiers.insert(application_id.to_string(), notifier);

        Ok(())
    }

    fn delete_notifier(&self, id: &str) {
        if self.notifiers.write().unwrap().remove(id).is_some() {
            self.set_app_counter();
            warn!("Application removed"; "universe" => id);
        }
    }

    fn set_app_counter(&self) {
        NUMBER_OF_APPLICATIONS.set(self.notifiers.read().unwrap().len() as f64);
    }
}

impl EventHandler for ApnsHandler {
    fn accepts(&self, event: &PushNotification) -> bool {
        event.has_apple()
    }

    fn handle_notification(
        &self,
        key: Option<Vec<u8>>,
        event: PushNotification,
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send + Unpin> {
        let producer = self.producer.clone();
        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

        CALLBACKS_INFLIGHT.inc();

        if let Some(notifier) = self.notifiers.read().unwrap().get(event.get_universe()) {
            let dat = notifier.clone();
            let mute = Arc::new(Mutex::<Option<Result<(),()>>>::new(None));
            let ret = FutureMime::new(mute.clone());
            thread::spawn(|| async  move{
                let notification_send = dat.notify(event.clone());
                let b = notification_send.then(move |result| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();
                        match result {
                            Ok(x) => {
                                match x {
                                    Ok(x) => {
                                        if let Some(t) = x.error {
                                            producer.handle_err_reason(key, event, &t.reason)
                                        }
                                        else {
                                            producer.handle_ok(key, event)
                                        }
                                    },
                                    Err(e) => producer.handle_err(key, event, &e)
                                }
                                //
                            },
                            //Ok(Err(e)) => ,
                            Err(_) => producer.handle_fatal(key, event),
                        }
                    }).then(|_| ok(())).await;
                *mute.lock().unwrap() = Some(b);
                }
            );


            Box::new(ret)

        } else {
            let connection_error = producer
                .handle_fatal(key, event)//BLAH need to fiiiix
                .then(|_| err(()));

            Box::new(connection_error)
        }
    }

    fn handle_http(
        &self,
        _: Option<Vec<u8>>,
        _: HttpRequest,
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send + Unpin> {
        warn!("We don't handle http request events here");
        Box::new(err(()))
    }

    fn handle_config(&self, id: &str, application: Option<Application>) {
        match application {
            None => {
                self.delete_notifier(id);
            }
            Some(application) => {
                let application_id = application.get_id();

                if !application.has_ios_config() {
                    debug!("No ios config"; &application);
                    self.delete_notifier(application_id);
                    return;
                }

                let ios_config = application.get_ios_config();

                if !ios_config.get_enabled() {
                    debug!("Not enabled"; &application);
                    self.delete_notifier(application_id);
                    return;
                }

                if !ios_config.has_token() && !ios_config.has_certificate() {
                    debug!("No connection details"; &application);
                    self.delete_notifier(application_id);
                    return;
                }

                let result = if ios_config.has_token() {
                    let token_config = ios_config.get_token();

                    let endpoint = match token_config.get_endpoint() {
                        Production => Endpoint::Production,
                        Sandbox => Endpoint::Sandbox,
                    };

                    info!(
                        "Updating application configuration";
                        &application,
                        "connection_type" => "token",
                        "team_id" => token_config.get_team_id(),
                        "key_id" => token_config.get_key_id(),
                        "apns_topic" => token_config.get_apns_topic(),
                        "endpoint" => format!("{:?}", endpoint)
                    );

                    self.add_token_notifier(
                        token_config,
                        endpoint,
                        application_id,
                        token_config.get_apns_topic(),
                    )
                } else {
                    let cert_config = ios_config.get_certificate();

                    let endpoint = match cert_config.get_endpoint() {
                        Production => Endpoint::Production,
                        Sandbox => Endpoint::Sandbox,
                    };

                    info!(
                        "Updating application configuration";
                        &application,
                        "connection_type" => "certificate",
                        "apns_topic" => cert_config.get_apns_topic(),
                        "endpoint" => format!("{:?}", endpoint)
                    );

                    self.add_certificate_notifier(
                        cert_config,
                        endpoint,
                        application_id,
                        cert_config.get_apns_topic(),
                    )
                };

                self.set_app_counter();

                if let Err(error) = result {
                    error!(
                        "Error connecting to APNs";
                        &application,
                        "error" => format!("{:?}", error)
                    )
                };
            }
        }
    }
}
