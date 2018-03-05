use std::sync::{Arc, Mutex};
use config::Config;
use std::sync::atomic::{AtomicBool, Ordering};
use certificate_registry::CertificateRegistry;
use std::time::Duration;
use std::thread::JoinHandle;
use time::Timespec;
use apns2::error;
use std::collections::HashMap;
use std::thread;
use certificate_registry::{ApnsConnectionParameters, Application, CertificateError};
use consumer::Consumer;
use logger::{GelfLogger, LogAction};
use gelf::{Level as GelfLevel, Message as GelfMessage};
use futures::sync::oneshot;
use futures::Future;
use futures::sync::mpsc;
use producer::ApnsData;

pub static MAX_FAILURES: i32 = 10;

lazy_static! {
    pub static ref CONSUMER_FAILURES: Mutex<HashMap<i32, i32>> = Mutex::new(HashMap::new());
}

pub struct ConsumerSupervisor {
    config: Arc<Config>,
    control: Arc<AtomicBool>,
    certificate_registry: CertificateRegistry,
    wait_duration: Duration,
    consumers: HashMap<i32, ApplicationConsumer>,
    logger: Arc<GelfLogger>,
    producer_tx: mpsc::Sender<ApnsData>,
}

struct ApplicationConsumer {
    handle: Option<JoinHandle<()>>,
    updated_at: Option<Timespec>,
    control: oneshot::Sender<()>,
}

impl Drop for ConsumerSupervisor {
    fn drop(&mut self) {
        for (_, mut consumer) in self.consumers.drain() {
            consumer.control.send(()).unwrap();
            if let Some(handle) = consumer.handle.take() {
                handle.thread().unpark();
                handle.join().unwrap();
            }
        }
    }
}

enum ConsumerAction {
    Update(ApplicationConsumer),
    NoUpdate,
}

impl ConsumerSupervisor {
    pub fn new(
        config: Arc<Config>,
        control: Arc<AtomicBool>,
        producer_tx: mpsc::Sender<ApnsData>,
        logger: Arc<GelfLogger>,
    ) -> ConsumerSupervisor {
        let registry = CertificateRegistry::new(config.clone());

        ConsumerSupervisor {
            config: config,
            control: control,
            certificate_registry: registry,
            wait_duration: Duration::from_secs(5),
            consumers: HashMap::new(),
            logger: logger,
            producer_tx: producer_tx,
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
                }
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
                    }
                    _ => match self.create_consumer(app.clone()) {
                        Ok(consumer) => {
                            acc.insert(app.id, ConsumerAction::Update(consumer));
                        }
                        Err(e) => {
                            let mut failures = CONSUMER_FAILURES.lock().unwrap();
                            failures.insert(app.id, MAX_FAILURES);

                            let mut log_msg =
                                GelfMessage::new(format!("Error in creating a consumer"));
                            let _ = log_msg.set_metadata("app_id", format!("{}", app.id));
                            let _ = log_msg.set_metadata("successful", format!("false"));
                            let _ = log_msg
                                .set_metadata("action", format!("{:?}", LogAction::ConsumerCreate));

                            log_msg.set_full_message(format!("{:?}", e));
                            log_msg.set_level(GelfLevel::Error);

                            self.logger.log_message(log_msg);
                        }
                    },
                }

                acc
            })
        })
    }

    fn remove_failed(&mut self) {
        let mut failures = CONSUMER_FAILURES.lock().unwrap();

        for (app_id, failures) in failures.iter_mut() {
            if *failures >= MAX_FAILURES {
                let mut log_msg =
                    GelfMessage::new(String::from("Supervisor consumer failure and restart"));
                let _ = log_msg.set_metadata("app_id", format!("{}", app_id));
                let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerRestart));

                log_msg.set_full_message(String::from(
                    "Consumer connection to APNS is unstable and the consumer is restarted.",
                ));
                log_msg.set_level(GelfLevel::Informational);
                self.logger.log_message(log_msg);

                if let Some(consumer) = self.consumers.remove(&app_id) {
                    consumer.control.send(()).unwrap();
                }

                *failures = 0;
            }
        }
    }

    fn drop_deleted(&mut self, updates: &HashMap<i32, ConsumerAction>) {
        for (ref app_id, _) in self.consumers.iter() {
            if !updates.contains_key(app_id) {
                let mut log_msg = GelfMessage::new(String::from("Supervisor consumer delete"));
                let _ = log_msg.set_metadata("app_id", format!("{}", app_id));
                let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerDelete));

                log_msg.set_full_message(String::from(
                    "Consumer is disabled, application deleted or certificate expired",
                ));
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
                }
                _ => (),
            }
        }
    }

    fn create_consumer(&self, app: Application) -> Result<ApplicationConsumer, error::Error> {
        let (control_tx, control_rx) = oneshot::channel();

        let mut log_msg = GelfMessage::new(String::from("Supervisor consumer update"));
        let _ = log_msg.set_metadata("app_id", format!("{}", app.id));
        let _ = log_msg.set_metadata("action", format!("{:?}", LogAction::ConsumerCreate));

        log_msg.set_level(GelfLevel::Informational);
        log_msg.set_full_message(String::from("A new consumer is created"));

        match app.connection_parameters {
            ApnsConnectionParameters::Certificate {
                pkcs12: _,
                password: _,
                ref endpoint,
            } => {
                let _ = log_msg.set_metadata("endpoint", format!("{:?}", endpoint));
                let _ = log_msg.set_metadata("connection_type", String::from("certificate"));
            }
            ApnsConnectionParameters::Token {
                pkcs8: _,
                ref key_id,
                ref team_id,
                ref endpoint,
            } => {
                let _ = log_msg.set_metadata("key_id", format!("{}", key_id));
                let _ = log_msg.set_metadata("team_id", format!("{}", team_id));
                let _ = log_msg.set_metadata("endpoint", format!("{:?}", endpoint));
                let _ = log_msg.set_metadata("connection_type", String::from("token"));
            }
        };

        let handle = {
            let config = self.config.clone();
            let logger = self.logger.clone();
            let app = app.clone();
            let app_id = app.id;
            let control = control_rx.shared();
            let producer_tx = self.producer_tx.clone();

            thread::spawn(move || {
                let consumer = Consumer::new(config, app, logger);

                match consumer.consume(control, producer_tx) {
                    Ok(_) => info!("Consumer #{} exited normally", app_id),
                    Err(e) => error!("Couldn't start consumer #{}: {:?}", app_id, e),
                }
            })
        };

        self.logger.log_message(log_msg);

        Ok(ApplicationConsumer {
            handle: Some(handle),
            updated_at: app.updated_at,
            control: control_tx,
        })
    }
}
