use hyper::client::{Client, Response};
use hyper::error::{Error as HttpError};
use hyper::status::StatusCode;

pub struct ArtifactoryClient {
    client: Client,
    pub base_uri: String,
}

#[derive(Debug)]
pub enum ArtifactoryError {
    Hyper(HttpError),
    Status(StatusCode),
}

impl ArtifactoryClient {
    pub fn new(base_uri: String) -> ArtifactoryClient {
        ArtifactoryClient {
            client: Client::new(),
            base_uri: base_uri,
        }
    }

    pub fn fetch_certificate(&self, folder: &str) -> Result<Response, ArtifactoryError> {
        let uri = format!("{}/apns_keys/{}/push_cert.pem", self.base_uri, folder);
        info!("Fetching certificate from uri: {}", uri);

        self.fetch(uri)
    }

    pub fn fetch_private_key(&self, folder: &str) -> Result<Response, ArtifactoryError> {
        let uri = format!("{}/apns_keys/{}/push_cert.key", self.base_uri, folder);
        info!("Fetching key from uri: {}", uri);

        self.fetch(uri)
    }

    fn fetch(&self, uri: String) -> Result<Response, ArtifactoryError> {
        match self.client.get(&uri).send() {
            Ok(response) =>
                if response.status == StatusCode::Ok {
                    Ok(response)
                } else {
                    Err(ArtifactoryError::Status(response.status))
                },
            Err(e) => Err(ArtifactoryError::Hyper(e)),
        }
    }
}
