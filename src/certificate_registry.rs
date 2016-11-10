use std::sync::Arc;
use config::Config;
use std::io::Cursor;
use time::Timespec;
use postgres::error::{Error as PostgresError};
use r2d2;
use r2d2_postgres::{SslMode, PostgresConnectionManager};
use std::time::Duration;

#[derive(Debug)]
pub enum CertificateError {
    Postgres(PostgresError),
    NotFoundError(String),
    NotChanged(String),
    ApplicationIdError(String),
}

pub struct CertificateRegistry {
    pool: r2d2::Pool<PostgresConnectionManager>
}

pub struct CertificateData<'a> {
    pub certificate: Cursor<&'a [u8]>,
    pub private_key: Cursor<&'a [u8]>,
    pub updated_at: Option<Timespec>,
    pub apns_topic: Option<String>,
    pub is_sandbox: bool
}


impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let manager = PostgresConnectionManager::new(config.postgres.uri.as_str(), SslMode::None).unwrap();

        let psql_config = r2d2::Config::builder()
            .pool_size(config.postgres.pool_size)
            .min_idle(Some(config.postgres.min_idle))
            .idle_timeout(Some(Duration::from_millis(config.postgres.idle_timeout)))
            .max_lifetime(Some(Duration::from_millis(config.postgres.max_lifetime)))
            .build();

        CertificateRegistry {
            pool: r2d2::Pool::new(psql_config, manager).unwrap(),
        }
    }

    pub fn with_certificate<T, F>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(CertificateData) -> Result<T, CertificateError> {

        info!("Loading certificates from database for {}", application);

        let query = "SELECT ios.certificate, ios.private_key, ios.apns_topic, ios.updated_at, ios.is_sandbox \
                     FROM ios_applications ios \
                     INNER JOIN applications app ON app.id = ios.application_id \
                     WHERE ios.application_id = $1 \
                     AND ios.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND ios.certificate IS NOT NULL AND ios.private_key IS NOT NULL \
                     LIMIT 1";

        let connection = self.pool.get().unwrap();

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
                        is_sandbox: row.get("is_sandbox"),
                    })
                }
            },
            Err(e) => Err(CertificateError::Postgres(e)),
        }
    }
}
