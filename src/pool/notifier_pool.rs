use certificate_registry::{CertificateRegistry, CertificateError, CertificateData};
use notifier::*;
use std::sync::Arc;
use time::{precise_time_s, Timespec};
use std::collections::HashMap;

pub struct Notifier {
    pub apns: Option<CertificateNotifier>,
    pub topic: String,
    pub updated_at: Option<Timespec>,
    pub timestamp: f64,
}

pub struct NotifierPool {
    certificate_registry: Arc<CertificateRegistry>,
    notifiers: HashMap<String, Notifier>,
    cache_ttl: f64,
}

impl NotifierPool {
    pub fn new(certificate_registry: Arc<CertificateRegistry>) -> NotifierPool {
        NotifierPool {
            certificate_registry: certificate_registry,
            notifiers: HashMap::new(),
            cache_ttl: 240.0,
        }
    }

    pub fn get(&mut self, application_id: &str) -> Option<&Notifier> {
        self.notifiers.get(application_id)
    }

    pub fn update(&mut self, application_id: &str) {
        if self.notifiers.get(application_id).is_some() {
            if self.is_expired(self.notifiers.get(application_id).unwrap().timestamp) {
                self.update_expired(application_id);
            }
        } else {
            self.create_new(application_id);
        }
    }

    fn create_new(&mut self, application_id: &str) {
        let create_notifier = move |cert: CertificateData| {
            Ok(Notifier {
                apns: Some(CertificateNotifier::new(cert.certificate, cert.private_key, cert.is_sandbox)),
                topic: cert.apns_topic,
                updated_at: cert.updated_at,
                timestamp: precise_time_s(),
            })
        };

        match self.certificate_registry.with_certificate(application_id, create_notifier) {
            Ok(notifier) => {
                self.notifiers.insert(application_id.to_string(), notifier);
            },
            Err(e) => {
                error!("Error when fetching certificate for {}: {:?}", application_id, e);

                self.notifiers.insert(application_id.to_string(), Notifier {
                    apns: None,
                    topic: String::new(),
                    updated_at: None,
                    timestamp: precise_time_s(),
                });
            }
        }
    }

    fn update_expired(&mut self, application_id: &str) {
        let last_update = self.notifiers.get(application_id).unwrap().updated_at.clone();
        let mut ping_result = None;

        if let Some(ref apns) = self.notifiers.get(application_id).unwrap().apns {
            ping_result = Some(apns.client.client.ping());
        }

        let create_notifier = move |cert: CertificateData| {
            if cert.updated_at != last_update {
                Ok(Notifier {
                    apns: Some(CertificateNotifier::new(cert.certificate, cert.private_key, cert.is_sandbox)),
                    topic: cert.apns_topic,
                    updated_at: cert.updated_at,
                    timestamp: precise_time_s(),
                })
            } else {
                match ping_result {
                    Some(Ok(())) => Err(CertificateError::NotChanged(format!("No changes to the certificate, ping ok"))),
                    None => Err(CertificateError::NotChanged(format!("No changes to the certificate"))),
                    Some(Err(e)) => {
                        error!("Error when pinging apns, reconnecting: {}", e);

                        Ok(Notifier {
                            apns: Some(CertificateNotifier::new(cert.certificate, cert.private_key, cert.is_sandbox)),
                            topic: cert.apns_topic,
                            updated_at: cert.updated_at,
                            timestamp: precise_time_s(),
                        })
                    }
                }
            }
        };

        match self.certificate_registry.with_certificate(&application_id, create_notifier) {
            Ok(notifier) => {
                self.notifiers.remove(application_id);
                self.notifiers.insert(application_id.to_string(), notifier);
                info!("New certificate for application {}", application_id);
            },
            Err(CertificateError::NotChanged(s)) => {
                let mut notifier = self.notifiers.get_mut(application_id).unwrap();
                notifier.timestamp = precise_time_s();

                info!("Alles gut for application {}: {:?}", application_id, s);
            },
            Err(e) => {
                error!("Error when fetching certificate for {}, removing: {:?}", application_id, e);

                let mut notifier = self.notifiers.get_mut(application_id).unwrap();
                notifier.timestamp = precise_time_s();
                notifier.apns = None;
                notifier.topic = String::new();
                notifier.updated_at = None;
            }
        }
    }

    fn is_expired(&self, ts: f64) -> bool {
        precise_time_s() - ts >= self.cache_ttl
    }
}
