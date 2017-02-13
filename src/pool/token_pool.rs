use std::collections::HashMap;
use certificate_registry::{CertificateRegistry, CertificateError, TokenData};
use std::sync::Arc;
use time::{precise_time_s, Timespec};
use apns2::apns_token::APNSToken;

pub struct Token {
    pub apns: Option<APNSToken>,
    pub sandbox: bool,
    pub topic: String,
    pub updated_at: Option<Timespec>,
    pub timestamp: f64,
}

pub struct TokenPool {
    certificate_registry: Arc<CertificateRegistry>,
    tokens: HashMap<String, Token>,
    cache_ttl: f64,
}

impl TokenPool {
    pub fn new(certificate_registry: Arc<CertificateRegistry>) -> TokenPool {
        TokenPool {
            certificate_registry: certificate_registry,
            tokens: HashMap::new(),
            cache_ttl: 60.0,
        }
    }

    pub fn get(&mut self, application_id: &str) -> Option<&Token> {
        self.tokens.get(application_id)
    }

    pub fn update(&mut self, application_id: &str) {
        match self.tokens.get_mut(application_id) {
            Some(ref mut token) => match token.apns {
                Some(ref mut apns) if apns.is_expired() => {
                    info!("Token for application {} is expired, renewing", application_id);
                    apns.renew().unwrap();
                    return;
                },
                _ => ()
            },
            _ => (),
        }

        if self.tokens.get(application_id).is_some() && self.is_expired(self.tokens.get(application_id).unwrap().timestamp) {
            self.update_existing(application_id);
        } else if self.tokens.get(application_id).is_none() {
            self.create_new(application_id);
        }
    }

    fn update_existing(&mut self, application_id: &str) {
        let last_update = self.tokens.get(application_id).unwrap().updated_at.clone();

        let create_token = move |token: TokenData| {
            if token.updated_at != last_update {
                Ok(Token {
                    apns: Some(APNSToken::new(token.private_key, token.key_id, token.team_id).unwrap()),
                    sandbox: token.is_sandbox,
                    topic: token.apns_topic,
                    updated_at: token.updated_at,
                    timestamp: precise_time_s(),
                })
            } else {
                Err(CertificateError::NotChanged(format!("No changes to the token")))
            }
        };

        match self.certificate_registry.with_token(&application_id, create_token) {
            Ok(token) => {
                self.tokens.remove(application_id);
                self.tokens.insert(application_id.to_string(), token);
                info!("New token for application {}", application_id);
            }
            Err(CertificateError::NotChanged(s)) => {
                let mut token = self.tokens.get_mut(application_id).unwrap();
                token.timestamp = precise_time_s();

                info!("Alles gut for application {}: {:?}", application_id, s);
            },
            Err(e) => {
                error!("Now this error here, we HAD a token for an application {}, \
                        but right now we don't anymore. Either way, the customer \
                        switched to certificate authentication or the application \
                        is now disabled. The reason was: {:?}", application_id, e);

                let mut token = self.tokens.get_mut(application_id).unwrap();
                token.timestamp = precise_time_s();
                token.topic = String::new();
                token.apns = None;
                token.updated_at = None;
            }
        }
    }

    fn create_new(&mut self, application_id: &str) {
        let create_token = move |token: TokenData| {
            Ok(Token {
                apns: Some(APNSToken::new(token.private_key, token.key_id, token.team_id).unwrap()),
                sandbox: token.is_sandbox,
                topic: token.apns_topic,
                updated_at: token.updated_at,
                timestamp: precise_time_s(),
            })
        };

        match self.certificate_registry.with_token(application_id, create_token) {
            Ok(token) => {
                self.tokens.insert(application_id.to_string(), token);
            },
            Err(e) => {
                warn!("Ok, so there was no token auth enabled for the application {}. \
                       This is nothing serious, they probably just use certificate \
                       authentication. The reason was: {:?}", application_id, e);

                self.tokens.insert(application_id.to_string(), Token {
                    apns: None,
                    sandbox: true,
                    topic: String::new(),
                    updated_at: None,
                    timestamp: precise_time_s(),
                });
            }
        }
    }

    fn is_expired(&self, ts: f64) -> bool {
        precise_time_s() - ts >= self.cache_ttl
    }
}
