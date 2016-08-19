use std::sync::Arc;
use config::Config;
use mysql::{Pool, Opts, Error as MysqlError};

pub struct CertificateRegistry {
    mysql: Pool,
}

#[derive(Debug)]
pub enum CertificateError {
    Mysql(MysqlError),
    NotFoundError(String),
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let opts = Opts::from_url(&config.mysql.uri).unwrap();

        CertificateRegistry {
            mysql: Pool::new(opts).unwrap(),
        }
    }

    pub fn fetch<F, T>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(&str) -> T {

        let query = "SELECT api_key \
                     FROM android_applications droid \
                     INNER JOIN applications app on app.id = droid.application_id \
                     WHERE droid.app_store_id = :app_store_id \
                     AND droid.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND droid.api_key IS NOT NULL";

        info!("Loading api_key from mysql to {}", application);

        match self.mysql.first_exec(query, params!{"app_store_id" => application}) {
            Ok(Some(mut row)) => {
                let api_key: String = row.take("api_key").unwrap();

                Ok(f(&api_key))
            },
            Ok(None) => Err(CertificateError::NotFoundError(format!("Couldn't find a certificate for {}", application))),
            Err(e) => Err(CertificateError::Mysql(e)),
        }
    }
}
