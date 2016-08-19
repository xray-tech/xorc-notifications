use std::sync::Arc;
use config::Config;
use mysql::{Pool, Opts, Error as MysqlError};
use std::io::Cursor;
use time::Timespec;

#[derive(Debug)]
pub enum CertificateError {
    Mysql(MysqlError),
    NotFoundError(String),
    NotChanged(String),
}

pub struct CertificateRegistry {
    mysql: Pool
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let opts = Opts::from_url(&config.mysql.uri).unwrap();

        CertificateRegistry {
            mysql: Pool::new(opts).unwrap(),
        }
    }

    pub fn with_certificate<T, F>(&self, application: &str, f: F) -> Result<T, CertificateError>
        where F: FnOnce(Cursor<&[u8]>, Cursor<&[u8]>, Option<Timespec>) -> Result<T, CertificateError> {
        info!("Loading certificates from mysql for {}", application);

        let query = "SELECT certificate, private_key, ios.updated_at \
                     FROM ios_applications ios \
                     INNER JOIN applications app ON app.id = ios.application_id \
                     WHERE ios.app_store_id = :app_store_id \
                     AND ios.enabled IS TRUE AND app.deleted_at IS NULL \
                     AND ios.certificate IS NOT NULL AND ios.private_key IS NOT NULL";

        match self.mysql.first_exec(query, params!{"app_store_id" => application}) {
            Ok(Some(mut row)) => {
                let certificate: String = row.take("certificate").unwrap();
                let private_key: String = row.take("private_key").unwrap();
                let updated_at: Option<Timespec> = row.take("updated_at");

                f(Cursor::new(certificate.into_bytes().as_slice()),
                  Cursor::new(private_key.into_bytes().as_slice()),
                  updated_at)
            },
            Ok(None) => Err(CertificateError::NotFoundError(format!("Couldn't find a certificate for {}", application))),
            Err(e) => Err(CertificateError::Mysql(e)),
        }
    }
}
