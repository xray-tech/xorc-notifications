use std::sync::{Arc, Mutex};
use config::Config;
use std::sync::atomic::{AtomicBool, Ordering};
use certificate_registry::CertificateRegistry;
use std::time::Duration;
use std::sync::mpsc::Sender;
use producer::ApnsResponse;
use std::thread::JoinHandle;
use time::Timespec;
use notifier::{CertificateNotifier, TokenNotifier};
use apns2::apns_token::APNSToken;
use apns2::client::APNSError;
use std::collections::{HashSet, HashMap};
use std::thread;
use certificate_registry::{Application, ApnsConnectionParameters, ApnsEndpoint, CertificateError};
use consumer::Consumer;
use std::io::Cursor;
use logger::GelfLogger;
use gelf::{Message as GelfMessage, Level as GelfLevel};

lazy_static! {
    pub static ref CONSUMER_FAILURES: Mutex<HashSet<i32>> = Mutex::new(HashSet::new());
}

pub struct ConsumerSupervisor {
    config: Arc<Config>,
    control: Arc<AtomicBool>,
    certificate_registry: CertificateRegistry,
    wait_duration: Duration,
    tx_response: Sender<ApnsResponse>,
    consumers: HashMap<i32, ApplicationConsumer>,
    logger: Arc<GelfLogger>
}

struct ApplicationConsumer {
    handle: Option<JoinHandle<()>>,
    app_id: i32,
    control: Arc<AtomicBool>,
    updated_at: Option<Timespec>,
}

impl Drop for ApplicationConsumer {
    fn drop(&mut self) {
        self.control.store(false, Ordering::Relaxed);

        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            handle.join().unwrap();
        }

        info!("Consumer dropped for application #{}", self.app_id);
    }
}

pub enum ApnsConnection {
    Certificate { notifier: CertificateNotifier, topic: String },
    Token { notifier: TokenNotifier, token: APNSToken, topic: String }
}

enum ConsumerAction {
    Update(ApplicationConsumer),
    NoUpdate,
}

impl ConsumerSupervisor {
    pub fn new(config: Arc<Config>, control: Arc<AtomicBool>, tx_response: Sender<ApnsResponse>, logger: Arc<GelfLogger>) -> ConsumerSupervisor {
        let registry = CertificateRegistry::new(config.clone());

        ConsumerSupervisor {
            config: config,
            control: control,
            certificate_registry: registry,
            wait_duration: Duration::from_secs(5),
            tx_response: tx_response,
            consumers: HashMap::new(),
            logger: logger,
        }
    }

    pub fn run(&mut self) {
        while self.control.load(Ordering::Relaxed) {
            self.remove_failed();

            match self.fetch_updates() {
                Ok(updates) => {
                    self.drop_deleted(&updates);
                    self.update_existing(updates);

                    thread::park_timeout(self.wait_duration);
                },
                Err(e) => {
                    error!("Error in fetching application data: {:?}", e);
                    self.control.store(false, Ordering::Relaxed);
                }
            }

        }
    }

    fn fetch_updates(&self) -> Result<HashMap<i32, ConsumerAction>, CertificateError> {
        self.certificate_registry.with_apps(|apps| {
            apps.iter().fold(HashMap::new(), |mut acc, app| {
                match self.consumers.get(&app.id) {
                    Some(consumer) if consumer.updated_at == app.updated_at => {
                        acc.insert(app.id, ConsumerAction::NoUpdate);
                    },
                    _ => {
                        match self.create_consumer(&app) {
                            Ok(consumer) => {
                                acc.insert(app.id, ConsumerAction::Update(consumer));
                            },
                            Err(e) => {
                                error!("Error in creating a consumer for app {}: {:?}", app.id, e);
                            }
                        }
                    },
                }

                acc
            })
        })
    }

    fn remove_failed(&mut self) {
        let mut failures = CONSUMER_FAILURES.lock().unwrap();

        for app_id in failures.drain() {
            let mut log_msg = GelfMessage::new(String::from("Supervisor consumer failure and restart"));
            let _ = log_msg.set_metadata("app_id", format!("{}", app_id));

            log_msg.set_full_message(String::from("Consumer connection to APNS is unstable and the consumer is restarted."));
            log_msg.set_level(GelfLevel::Informational);
            self.logger.log_message(log_msg);

            self.consumers.remove(&app_id);
        }
    }

    fn drop_deleted(&mut self, updates: &HashMap<i32, ConsumerAction>) {
        for (ref app_id, _) in self.consumers.iter() {
            if !updates.contains_key(app_id) {
                let mut log_msg = GelfMessage::new(String::from("Supervisor consumer delete"));
                let _ = log_msg.set_metadata("app_id", format!("{}", app_id));

                log_msg.set_full_message(String::from("Consumer is disabled, application deleted or certificate expired"));
                log_msg.set_level(GelfLevel::Informational);
                self.logger.log_message(log_msg);
            }
        }

        self.consumers.retain(|ref id, _| {
            if updates.contains_key(id) {
                true
            } else {
                false
            }
        });
    }

    fn update_existing(&mut self, updates: HashMap<i32, ConsumerAction>) {
        for (id, update) in updates {
            match update {
                ConsumerAction::Update(consumer) => {
                    self.consumers.insert(id, consumer);
                },
                _ => ()
            }
        };
    }

    fn create_consumer(&self, app: &Application) -> Result<ApplicationConsumer, APNSError> {
        let control = Arc::new(AtomicBool::new(true));
        let mut log_msg = GelfMessage::new(String::from("Supervisor consumer update"));
        let _ = log_msg.set_metadata("app_id", format!("{}", app.id));

        log_msg.set_level(GelfLevel::Informational);
        log_msg.set_full_message(String::from("A new consumer is created"));

        let connection = match app.connection_parameters {
            ApnsConnectionParameters::Certificate { ref certificate, ref private_key, ref endpoint } => {
                let _ = log_msg.set_metadata("endpoint", format!("{:?}", endpoint));
                let _ = log_msg.set_metadata("connection_type", String::from("certificate"));

                ApnsConnection::Certificate {
                    notifier: CertificateNotifier::new(
                        Cursor::new(certificate),
                        Cursor::new(private_key),
                        endpoint == &ApnsEndpoint::Sandbox)?,
                    topic: app.topic.clone(),
                }
            },
            ApnsConnectionParameters::Token { ref token, ref key_id, ref team_id, ref endpoint } => {
                let _ = log_msg.set_metadata("key_id", format!("{}", key_id));
                let _ = log_msg.set_metadata("team_id", format!("{}", team_id));
                let _ = log_msg.set_metadata("endpoint", format!("{:?}", endpoint));
                let _ = log_msg.set_metadata("connection_type", String::from("token"));

                ApnsConnection::Token {
                    notifier: TokenNotifier::new(
                        endpoint == &ApnsEndpoint::Sandbox,
                        self.config.clone()),
                    token: APNSToken::new(
                        Cursor::new(token),
                        key_id.as_ref(),
                        team_id.as_ref(), 1200).unwrap(),
                    topic: app.topic.clone(),
                }
            }
        };

        let mut consumer = Consumer::new(control.clone(), self.config.clone(), self.logger.clone(), app.id);
        let tx_response = self.tx_response.clone();

        let handle = thread::spawn(move || {
            match consumer.consume(connection, tx_response) {
                Ok(()) => {
                    info!("Consumer thread exited.");
                },
                Err(e) => {
                    error!("Error in consumer thread: {:?}", e);
                }
            }
        });

        self.logger.log_message(log_msg);

        Ok(ApplicationConsumer {
            handle: Some(handle),
            control: control,
            app_id: app.id,
            updated_at: app.updated_at,
        })
    }
}
