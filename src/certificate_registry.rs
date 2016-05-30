use std::collections::HashMap;
use std::sync::{Mutex, Arc};
use config::Config;
use artifactory::{ArtifactoryClient, ArtifactoryError};
use std::io::{Read, Cursor};

pub struct CertificateRegistry {
    certificates: Mutex<HashMap<String, (Vec<u8>, Vec<u8>)>>,
    artifactory: ArtifactoryClient,
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        CertificateRegistry {
            certificates: Mutex::new(HashMap::new()),
            artifactory: ArtifactoryClient::new(config.artifactory.base_uri.clone()),
        }
    }

    pub fn with_certificate<T, F>(&self, application: &str, f: F) -> Result<T, ArtifactoryError>
        where F: FnOnce(Cursor<&[u8]>, Cursor<&[u8]>) -> T {
            let mut certificates = self.certificates.lock().unwrap();

            if let Some(cert_pair) = certificates.get_mut(application) {
                return Ok(f(Cursor::new(cert_pair.0.as_slice()), Cursor::new(cert_pair.1.as_slice())))
            }

            let mut cert_data: Vec<u8> = Vec::new();
            let mut key_data: Vec<u8> = Vec::new();

            let mut cert = try!(self.artifactory.fetch_certificate(application));
            cert.read_to_end(&mut cert_data).unwrap();
            cert_data.shrink_to_fit();

            let mut key = try!(self.artifactory.fetch_private_key(application));
            key.read_to_end(&mut key_data).unwrap();
            key_data.shrink_to_fit();

            certificates.insert(String::from(application), (cert_data, key_data));

            let cert_pair = certificates.get_mut(application).unwrap();

            Ok(f(Cursor::new(cert_pair.0.as_slice()), Cursor::new(cert_pair.1.as_slice())))
        }
}
