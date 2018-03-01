use std::sync::Arc;
use config::Config;
use postgres::error::{Error as PostgresError};
use r2d2;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use std::time::Duration;

pub struct CertificateRegistry {
    pool: r2d2::Pool<PostgresConnectionManager>,
}

#[derive(Debug)]
pub enum CertificateError {
    Postgres(PostgresError),
    NotFoundError(String),
    ApplicationIdError(String),
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let manager = PostgresConnectionManager::new(config.postgres.uri.as_str(), TlsMode::None)
            .expect("Couldn't connect to PostgreSQL");

        let pool = r2d2::Builder::new()
            .max_size(config.postgres.pool_size)
            .min_idle(Some(config.postgres.min_idle))
            .idle_timeout(Some(Duration::from_millis(config.postgres.idle_timeout)))
            .max_lifetime(Some(Duration::from_millis(config.postgres.max_lifetime)))
            .build(manager).expect("Couldn't create a PostgreSQL connection pool");

        CertificateRegistry { pool }
    }

    pub fn fetch<F, T>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(&str) -> T {

        info!("Loading api_key from database to {}", application);

        let query = "SELECT api_key \
                     FROM android_applications droid \
                     INNER JOIN applications app on app.id = droid.application_id \
                     WHERE droid.application_id = $1 \
                     AND droid.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND droid.api_key IS NOT NULL";

        let connection = self.pool.get().expect("Error when getting a PostgreSQL connection from the pool");

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
                    let api_key: String = row.get("api_key");

                    Ok(f(&api_key))
                }
            },
            Err(e) => Err(CertificateError::Postgres(e)),
        }
    }
}
