use std::sync::Arc;
use config::Config;
use certificate_registry::{CertificateRegistry};
use notifier::{TokenNotifier, CertificateNotifier};
use apns2::apns_token::APNSToken;
use pool::{NotifierPool, TokenPool, Token, Notifier};

pub struct ConnectionPool {
    notifier_pool: NotifierPool,
    token_pool: TokenPool,
}

pub enum ApnsConnection<'a> {
    WithCertificate { notifier: &'a CertificateNotifier, topic: &'a str },
    WithToken { notifier: &'a TokenNotifier, token: &'a APNSToken, topic: &'a str }
}

impl ConnectionPool {
    pub fn new(certificate_registry: Arc<CertificateRegistry>, config: Arc<Config>) -> ConnectionPool {
        ConnectionPool {
            notifier_pool: NotifierPool::new(certificate_registry.clone()),
            token_pool: TokenPool::new(certificate_registry.clone(), config.clone()),
        }
    }

    pub fn get(&mut self, application_id: &str) -> Option<ApnsConnection> {
        self.token_pool.update(application_id);
        self.notifier_pool.update(application_id);

        match self.token_pool.get(application_id) {
            Some(&Token { apns: Some(ref token), notifier: Some(ref notifier), topic: ref t, timestamp: _, updated_at: _ }) => {
                Some(ApnsConnection::WithToken {
                    notifier: notifier,
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
