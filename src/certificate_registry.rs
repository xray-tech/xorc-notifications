use std::sync::Arc;
use config::Config;
use std::io::Cursor;
use time::Timespec;
use postgres::error::{Error as PostgresError};
use r2d2;
use r2d2_postgres::{SslMode, PostgresConnectionManager};
use std::time::Duration;
use apns2::client::APNSError;

#[derive(Debug)]
pub enum CertificateError {
    Postgres(PostgresError),
    NotFoundError(String),
    NotChanged(String),
    ApplicationIdError(String),
    Connection(APNSError),
    Ssl(APNSError),
    Unknown(APNSError),
}

impl From<APNSError> for CertificateError {
    fn from(e: APNSError) -> CertificateError {
        match e {
            APNSError::ClientConnectError(_) => CertificateError::Connection(e),
            APNSError::SslError(_) => CertificateError::Ssl(e),
            _ => CertificateError::Unknown(e)
        }
    }
}

pub struct CertificateRegistry {
    pool: r2d2::Pool<PostgresConnectionManager>
}

pub struct CertificateData<'a> {
    pub certificate: Cursor<&'a [u8]>,
    pub private_key: Cursor<&'a [u8]>,
    pub updated_at: Option<Timespec>,
    pub apns_topic: String,
    pub is_sandbox: bool,
}

pub struct TokenData<'a> {
    pub private_key: Cursor<&'a [u8]>,
    pub updated_at: Option<Timespec>,
    pub apns_topic: String,
    pub team_id: String,
    pub key_id: String,
    pub is_sandbox: bool,
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let manager = PostgresConnectionManager::new(config.postgres.uri.as_str(), SslMode::None)
            .expect("Couldn't connect to PostgreSQL");

        let psql_config = r2d2::Config::builder()
            .pool_size(config.postgres.pool_size)
            .min_idle(Some(config.postgres.min_idle))
            .idle_timeout(Some(Duration::from_millis(config.postgres.idle_timeout)))
            .max_lifetime(Some(Duration::from_millis(config.postgres.max_lifetime)))
            .build();

        CertificateRegistry {
            pool: r2d2::Pool::new(psql_config, manager).expect("Couldn't create a PostgreSQL connection pool"),
        }
    }

    pub fn with_certificate<T, F>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(CertificateData) -> Result<T, CertificateError> {

        info!("Loading certificates from database for {}", application);

        let query = "SELECT ios.certificate, ios.private_key, ios.apns_topic, ios.updated_at, ios.cert_is_sandbox \
                     FROM ios_applications ios \
                     INNER JOIN applications app ON app.id = ios.application_id \
                     WHERE ios.application_id = $1 \
                     AND ios.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND ios.certificate IS NOT NULL AND ios.private_key IS NOT NULL \
                     AND ios.connection_type = 'certificate' \
                     LIMIT 1";

        let connection = self.pool.get().expect("Error getting a PostgreSQL connection from the pool");

        let result = match application.parse::<i32>() {
            Ok(application_id) => connection.query(query, &[&application_id]),
            Err(_) => return Err(CertificateError::ApplicationIdError(format!("Invalid application_id: '{}'", application))),
        };

        match result {
            Ok(rows) => {
                if rows.is_empty() {
                    Err(CertificateError::NotFoundError(format!("Couldn't find a certificate for {}", application)))
                } else {
                    let row = rows.get(0);

                    f(CertificateData {
                        certificate: Cursor::new(row.get_bytes("certificate").unwrap()),
                        private_key: Cursor::new(row.get_bytes("private_key").unwrap()),
                        updated_at: row.get("updated_at"),
                        apns_topic: row.get("apns_topic"),
                        is_sandbox: row.get("cert_is_sandbox"),
                    })
                }
            },
            Err(e) => Err(CertificateError::Postgres(e)),
        }
    }

    pub fn with_token<T, F>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(TokenData) -> Result<T, CertificateError> {

        info!("Loading tokens from database for {}", application);

        let query = "SELECT ios.token_der, ios.apns_topic, ios.updated_at, ios.token_is_sandbox, ios.key_id, ios.team_id \
                     FROM ios_applications ios \
                     INNER JOIN applications app ON app.id = ios.application_id \
                     WHERE ios.application_id = $1 \
                     AND ios.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND ios.token_der IS NOT NULL AND ios.key_id IS NOT NULL AND ios.team_id IS NOT NULL \
                     AND ios.connection_type = 'token' \
                     LIMIT 1";

        let connection = self.pool.get().unwrap();

        let result = match application.parse::<i32>() {
            Ok(application_id) => connection.query(query, &[&application_id]),
            Err(_) => return Err(CertificateError::ApplicationIdError(format!("Invalid application_id: '{}'", application))),
        };

        match result {
            Ok(rows) => {
                if rows.is_empty() {
                    Err(CertificateError::NotFoundError(format!("Couldn't find a token for {}", application)))
                } else {
                    let row = rows.get(0);

                    f(TokenData {
                        private_key: Cursor::new(row.get_bytes("token_der").unwrap()),
                        updated_at: row.get("updated_at"),
                        apns_topic: row.get("apns_topic"),
                        is_sandbox: row.get("token_is_sandbox"),
                        team_id: row.get("team_id"),
                        key_id: row.get("key_id"),
                    })
                }
            },
            Err(e) => Err(CertificateError::Postgres(e)),
        }
    }
}
