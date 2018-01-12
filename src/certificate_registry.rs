use std::sync::Arc;
use config::Config;
use time::Timespec;
use postgres::error::{Error as PostgresError};
use r2d2;
use r2d2_postgres::{TlsMode, PostgresConnectionManager};
use std::time::Duration;
use apns2::client::APNSError;

#[derive(Debug)]
pub enum CertificateError {
    Postgres(PostgresError),
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
    pool: r2d2::Pool<PostgresConnectionManager>,
    all_apps_query: &'static str,
}

#[derive(PartialEq, Debug)]
pub enum ApnsEndpoint {
    Sandbox,
    Production
}

#[derive(FromSql)]
enum IosConnectionType {
    Certificate,
    Token,
}

#[derive(Debug)]
pub enum ApnsConnectionParameters {
    Certificate {
        certificate: Vec<u8>,
        private_key: Vec<u8>,
        endpoint: ApnsEndpoint,
    },
    Token {
        token: Vec<u8>,
        key_id: String,
        team_id: String,
        endpoint: ApnsEndpoint,
    }
}

#[derive(Debug)]
pub struct Application {
    pub connection_parameters: ApnsConnectionParameters,
    pub topic: String,
    pub id: i32,
    pub updated_at: Option<Timespec>,
    pub thread_count: i32,
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let manager = PostgresConnectionManager::new(config.postgres.uri.as_str(), TlsMode::None)
            .expect("Couldn't connect to PostgreSQL");

        let all_apps = "SELECT ios.certificate, ios.private_key, ios.apns_topic, ios.updated_at, ios.cert_is_sandbox, \
                        ios.token_der, ios.token_is_sandbox, ios.key_id, ios.team_id, ios.connection_type, \
                        app.id, ios.thread_count
                        FROM applications app
                        INNER JOIN ios_applications ios ON app.id = ios.application_id \
                        WHERE ios.enabled IS TRUE AND app.deleted_at IS NULL \
                        AND ((ios.connection_type = 'certificate' AND ios.certificate IS NOT NULL AND ios.private_key IS NOT NULL AND ios.expiry_at > current_timestamp) \
                             OR (ios.connection_type = 'token' AND ios.token_der IS NOT NULL AND ios.key_id IS NOT NULL AND ios.team_id IS NOT NULL))";

        let psql_config = r2d2::Config::builder()
            .pool_size(config.postgres.pool_size)
            .min_idle(Some(config.postgres.min_idle))
            .idle_timeout(Some(Duration::from_millis(config.postgres.idle_timeout)))
            .max_lifetime(Some(Duration::from_millis(config.postgres.max_lifetime)))
            .build();

        CertificateRegistry {
            pool: r2d2::Pool::new(psql_config, manager).expect("Couldn't create a PostgreSQL connection pool"),
            all_apps_query: all_apps,
        }
    }

    pub fn with_apps<F, T>(&self, f: F) -> Result<T, CertificateError>
        where F: FnOnce(Vec<Application>) -> T {

        let get_endpoint = |is_sandbox: bool| {
            match is_sandbox {
                true => ApnsEndpoint::Sandbox,
                false => ApnsEndpoint::Production,
            }
        };

        let connection = self.pool.get().expect("Error getting a PostgreSQL connection from the pool");
        let applications = connection.query(self.all_apps_query, &[]).map(|rows| {
            rows.iter().map(|row| {
                let connection_type: String = String::from_utf8(row.get_bytes("connection_type").unwrap().to_vec()).unwrap();

                let connection_params = match connection_type.as_ref() {
                    "certificate" =>
                        ApnsConnectionParameters::Certificate {
                            certificate: row.get_bytes("certificate").unwrap().to_vec(),
                            private_key: row.get_bytes("private_key").unwrap().to_vec(),
                            endpoint: get_endpoint(row.get("cert_is_sandbox")),
                        },
                    _ =>
                        ApnsConnectionParameters::Token {
                            token: row.get_bytes("token_der").unwrap().to_vec(),
                            endpoint: get_endpoint(row.get("token_is_sandbox")),
                            key_id: row.get("key_id"),
                            team_id: row.get("team_id"),
                        }
                };

                Application {
                    connection_parameters: connection_params,
                    topic: row.get("apns_topic"),
                    id: row.get("id"),
                    updated_at: row.get("updated_at"),
                    thread_count: row.get("thread_count"),
                }
            }).collect()
        });

        match applications {
            Ok(apps) => Ok(f(apps)),
            Err(e) => Err(CertificateError::Postgres(e))
        }
    }
}
