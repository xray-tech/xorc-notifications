use std::sync::Arc;
use config::Config;
use time::Timespec;
use postgres::error::Error as PostgresError;
use r2d2;
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use std::time::Duration;
use apns2::error::Error;
use apns2::client::Endpoint;

#[derive(Debug)]
pub enum CertificateError {
    Postgres(PostgresError),
    Connection(Error),
    Ssl(Error),
    Unknown(Error),
}

impl From<Error> for CertificateError {
    fn from(e: Error) -> CertificateError {
        match e {
            Error::ConnectionError => CertificateError::Connection(e),
            Error::SignerError(_) => CertificateError::Ssl(e),
            _ => CertificateError::Unknown(e),
        }
    }
}

pub struct CertificateRegistry {
    pool: r2d2::Pool<PostgresConnectionManager>,
    all_apps_query: String,
}

#[derive(FromSql)]
enum IosConnectionType {
    Certificate,
    Token,
}

#[derive(Debug, Clone)]
pub enum ApnsConnectionParameters {
    Certificate {
        pkcs12: Vec<u8>,
        password: String,
        endpoint: Endpoint,
    },
    Token {
        pkcs8: Vec<u8>,
        key_id: String,
        team_id: String,
        endpoint: Endpoint,
    },
}

#[derive(Debug, Clone)]
pub struct Application {
    pub connection_parameters: ApnsConnectionParameters,
    pub topic: String,
    pub id: i32,
    pub updated_at: Option<Timespec>,
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let manager = PostgresConnectionManager::new(config.postgres.uri.as_str(), TlsMode::None)
            .expect("Couldn't connect to PostgreSQL");

        let mut all_apps = String::from(
            "SELECT ios.pkcs12, ios.pkcs12_password, ios.apns_topic, ios.updated_at, \
                ios.cert_is_sandbox, ios.token_p8, ios.token_is_sandbox, ios.key_id, \
                ios.team_id, ios.connection_type, app.id
                FROM applications app
                INNER JOIN ios_applications ios ON app.id = ios.application_id \
                WHERE ios.enabled IS TRUE AND app.deleted_at IS NULL \
                AND ((ios.connection_type = 'certificate' \
                    AND ios.certificate IS NOT NULL \
                    AND ios.private_key IS NOT NULL \
                    AND ios.expiry_at > current_timestamp) \
                    OR (ios.connection_type = 'token' \
                        AND ios.token_der IS NOT NULL \
                        AND ios.key_id IS NOT NULL \
                        AND ios.team_id IS NOT NULL))"
        );

        if let Some(ref whitelist) = config.general.application_whitelist {
            let ids: Vec<String> =
                whitelist.iter().map(|i| i.to_string()).collect();

            all_apps = format!(
                "{} AND ios.application_id IN ({})",
                all_apps,
                ids.join(", ")
            );
        };

        let psql_config = r2d2::Config::builder()
            .pool_size(config.postgres.pool_size)
            .min_idle(Some(config.postgres.min_idle))
            .idle_timeout(Some(Duration::from_millis(config.postgres.idle_timeout)))
            .max_lifetime(Some(Duration::from_millis(config.postgres.max_lifetime)))
            .build();

        CertificateRegistry {
            pool: r2d2::Pool::new(psql_config, manager)
                .expect("Couldn't create a PostgreSQL connection pool"),
            all_apps_query: all_apps,
        }
    }

    pub fn with_apps<F, T>(&self, f: F) -> Result<T, CertificateError>
    where
        F: FnOnce(Vec<Application>) -> T,
    {
        let get_endpoint = |is_sandbox: bool| match is_sandbox {
            true => Endpoint::Sandbox,
            false => Endpoint::Production,
        };

        let connection = self.pool
            .get()
            .expect("Error getting a PostgreSQL connection from the pool");
        let applications = connection.query(&self.all_apps_query, &[]).map(|rows| {
            rows.iter()
                .map(|row| {
                    let connection_type: String = String::from_utf8(
                        row.get_bytes("connection_type").unwrap().to_vec(),
                    ).unwrap();

                    let connection_params = match connection_type.as_ref() {
                        "certificate" => ApnsConnectionParameters::Certificate {
                            pkcs12: row.get_bytes("pkcs12").unwrap().to_vec(),
                            password: row.get("pkcs12_password"),
                            endpoint: get_endpoint(row.get("cert_is_sandbox")),
                        },
                        _ => ApnsConnectionParameters::Token {
                            pkcs8: row.get_bytes("token_p8").unwrap().to_vec(),
                            key_id: row.get("key_id"),
                            team_id: row.get("team_id"),
                            endpoint: get_endpoint(row.get("token_is_sandbox")),
                        },
                    };

                    Application {
                        connection_parameters: connection_params,
                        topic: row.get("apns_topic"),
                        id: row.get("id"),
                        updated_at: row.get("updated_at"),
                    }
                })
                .collect()
        });

        match applications {
            Ok(apps) => Ok(f(apps)),
            Err(e) => Err(CertificateError::Postgres(e)),
        }
    }
}
