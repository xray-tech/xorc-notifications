use hyper::client::{Client, Response};
use hyper::header::Connection;
use hyper::error::{Error as HttpError};
use hyper::status::StatusCode;
use std::io::Read;

pub struct ArtifactoryClient {
    client: Client,
    base_uri: String,
    certificate_path: String,
}

#[derive(Debug)]
pub enum ArtifactoryError {
    Hyper(HttpError),
    Status(StatusCode),
}

impl ArtifactoryClient {
    pub fn new(base_uri: String, certificate_path: String) -> ArtifactoryClient {
        ArtifactoryClient {
            client: Client::new(),
            base_uri: base_uri,
            certificate_path: certificate_path,
        }
    }

    pub fn fetch_api_key(&self, folder: &str) -> Result<String, ArtifactoryError> {
        let uri = format!("{}/{}/{}/api_key", self.base_uri, self.certificate_path, folder);
        info!("Fetching certificate from uri: {}", uri);

        let mut response = try!(self.fetch(uri));
        let mut api_key = String::new();

        response.read_to_string(&mut api_key).unwrap();

        Ok(String::from(api_key.trim()))
    }

    fn fetch(&self, uri: String) -> Result<Response, ArtifactoryError> {
        match self.client.get(&uri).header(Connection::close()).send() {
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

