use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use events::push_notification::PushNotification;
use notifier::Apns2Notifier;
use apns2::AsyncResponse;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use certificate_registry::{CertificateRegistry, CertificateError, CertificateData};
use std::sync::mpsc::Sender;
use producer::ApnsResponse;
use time::{precise_time_s, Timespec};
use metrics::CALLBACKS_INFLIGHT;

struct Notifier {
    apns: Option<Apns2Notifier>,
    updated_at: Option<Timespec>,
    timestamp: f64,
}

pub struct Consumer {
    channel: Mutex<Channel>,
    session: Session,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    certificate_registry: Arc<CertificateRegistry>,
    tx_response: Sender<ApnsResponse>,
    cache_ttl: f64,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.channel.lock().unwrap().close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl Consumer {
    pub fn new(control: Arc<AtomicBool>,
               config: Arc<Config>,
               certificate_registry: Arc<CertificateRegistry>,
               tx_response: Sender<(PushNotification, Option<AsyncResponse>)>) -> Consumer {

        let mut session = Session::new(Options {
            vhost: &config.rabbitmq.vhost,
            host: &config.rabbitmq.host,
            port: config.rabbitmq.port,
            login: &config.rabbitmq.login,
            password: &config.rabbitmq.password, .. Default::default()
        }).unwrap();

        let mut channel = session.open_channel(1).unwrap();

        channel.queue_declare(
            &*config.rabbitmq.queue,
            false, // passive
            true,  // durable
            false, // exclusive
            false, // auto_delete
            false, // nowait
            Table::new()).unwrap();

        channel.exchange_declare(
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).unwrap();

        channel.queue_bind(
            &*config.rabbitmq.queue,
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.routing_key,
            false, // nowait
            Table::new()).unwrap();

        Consumer {
            channel: Mutex::new(channel),
            session: session,
            control: control,
            config: config,
            certificate_registry: certificate_registry,
            tx_response: tx_response,
            cache_ttl: 60.0,
        }
    }

    pub fn consume(&self) -> Result<(), Error> {
        let mut notifiers: HashMap<String, Notifier> = HashMap::new();
        let mut channel = self.channel.lock().unwrap();

        while self.control.load(Ordering::Relaxed) {
            for result in channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    self.update_notifiers(&mut notifiers, event.get_application_id());

                    let response = match notifiers.get(event.get_application_id()) {
                        Some(&Notifier { apns: Some(ref apns), timestamp: _, updated_at: _ }) => Some(apns.send(&event)),
                        _ => None,
                    };

                    CALLBACKS_INFLIGHT.inc();
                    self.tx_response.send((event, response)).unwrap();
                } else {
                    error!("Broken protobuf data");
                }

                result.ack();

                if !self.control.load(Ordering::Relaxed) { break; }
            }

            thread::park_timeout(Duration::from_millis(100));
        }

        Ok(())
    }

    fn update_notifiers(&self, notifiers: &mut HashMap<String, Notifier>, application_id: &str) {
        if notifiers.get(application_id).is_some() && self.is_expired(notifiers.get(application_id).unwrap()){
            let last_update = notifiers.get(application_id).unwrap().updated_at.clone();

            let mut ping_result = None;

            if let Some(ref apns) = notifiers.get(application_id).unwrap().apns {
                ping_result = Some(apns.apns2_provider.client.ping());
            }

            let create_notifier = move |cert: CertificateData| {
                if cert.updated_at != last_update {
                    Ok(Notifier {
                        apns: Some(Apns2Notifier::new(cert.certificate, cert.private_key, cert.apns_topic, &cert.is_sandbox)),
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
                                apns: Some(Apns2Notifier::new(cert.certificate, cert.private_key, cert.apns_topic, &cert.is_sandbox)),
                                updated_at: cert.updated_at,
                                timestamp: precise_time_s(),
                            })
                        }
                    }
                }
            };

            match self.certificate_registry.with_certificate(&application_id, create_notifier) {
                Ok(notifier) => {
                    notifiers.remove(application_id);
                    notifiers.insert(application_id.to_string(), notifier);
                    info!("New certificate for application {}", application_id);
                },
                Err(CertificateError::NotChanged(s)) => {
                    let mut notifier = notifiers.get_mut(application_id).unwrap();
                    notifier.timestamp = precise_time_s();

                    info!("Alles gut for application {}: {:?}", application_id, s);
                },
                Err(e) => {
                    error!("Error when fetching certificate for {}, removing: {:?}", application_id, e);

                    let mut notifier = notifiers.get_mut(application_id).unwrap();
                    notifier.timestamp = precise_time_s();
                    notifier.apns = None;
                    notifier.updated_at = None;
                }
            }
        } else if notifiers.get(application_id).is_none() {
            let create_notifier = move |cert: CertificateData| {
                Ok(Notifier {
                    apns: Some(Apns2Notifier::new(cert.certificate, cert.private_key, cert.apns_topic, &cert.is_sandbox)),
                    updated_at: cert.updated_at,
                    timestamp: precise_time_s(),
                })
            };

            match self.certificate_registry.with_certificate(application_id, create_notifier) {
                Ok(notifier) => {
                    notifiers.insert(application_id.to_string(), notifier);
                },
                Err(e) => {
                    error!("Error when fetching certificate for {}: {:?}", application_id, e);

                    notifiers.insert(application_id.to_string(), Notifier {
                        apns: None,
                        updated_at: None,
                        timestamp: precise_time_s(),
                    });
                }
            }
        }
    }

    fn is_expired(&self, notifier: &Notifier) -> bool {
        precise_time_s() - notifier.timestamp >= self.cache_ttl
    }
}
