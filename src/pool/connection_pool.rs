use std::sync::Arc;
use config::Config;
use std::collections::HashMap;
use certificate_registry::{CertificateRegistry};
use notifier::{TokenNotifier, CertificateNotifier};
use time::precise_time_s;
use apns2::apns_token::APNSToken;
use pool::{NotifierPool, TokenPool, Token, Notifier};

pub struct ConnectionPool {
    notifier_pool: NotifierPool,
    token_pool: TokenPool,
    last_connection_test: f64,
    token_notifiers: HashMap<&'static str, TokenNotifier>,
    config: Arc<Config>,
    cache_ttl: f64,
}

pub enum ApnsConnection<'a> {
    WithCertificate { notifier: &'a CertificateNotifier, topic: &'a str },
    WithToken { notifier: &'a TokenNotifier, token: &'a APNSToken, topic: &'a str }
}

impl ConnectionPool {
    pub fn new(certificate_registry: Arc<CertificateRegistry>, config: Arc<Config>) -> ConnectionPool {
        let mut token_notifiers = HashMap::new();

        token_notifiers.insert("staging", TokenNotifier::new(true, config.clone()));
        token_notifiers.insert("production", TokenNotifier::new(false, config.clone()));

        ConnectionPool {
            config: config,
            notifier_pool: NotifierPool::new(certificate_registry.clone()),
            token_pool: TokenPool::new(certificate_registry.clone()),
            token_notifiers: token_notifiers,
            last_connection_test: precise_time_s(),
            cache_ttl: 120.0,
        }
    }

    pub fn get(&mut self, application_id: &str) -> Option<ApnsConnection> {
        self.token_pool.update(application_id);
        self.notifier_pool.update(application_id);

        match self.token_pool.get(application_id) {
            Some(&Token { apns: Some(ref token), topic: ref t, sandbox: is_sandbox, timestamp: _, updated_at: _ }) => {
                let stage = if is_sandbox { "staging" } else { "production" };

                if precise_time_s() - self.last_connection_test > self.cache_ttl {
                    match self.token_notifiers.get(stage).unwrap().client.client.ping() {
                        Ok(()) => info!("APNs token connection ping OK for {}", stage),
                        Err(e) => {
                            info!("Error with token connection ping for {}, reconnecting: {:?}", stage, e);

                            self.token_notifiers.remove(stage);
                            self.token_notifiers.insert(stage, TokenNotifier::new(is_sandbox, self.config.clone()));
                        }
                    }

                    self.last_connection_test = precise_time_s();
                }


                Some(ApnsConnection::WithToken {
                    notifier: self.token_notifiers.get(stage).unwrap(),
                    token: token,
                    topic: t,
                })
            },
            _ => match self.notifier_pool.get(application_id) {
                Some(&Notifier { apns: Some(ref apns), topic: ref t, timestamp: _, updated_at: _ }) =>
                    Some(ApnsConnection::WithCertificate {
                        notifier: apns,
                        topic: t,
                    }),
                _ =>
                    None,
            },
        }
    }
}
