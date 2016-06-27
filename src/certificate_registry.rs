use std::collections::HashMap;
use std::sync::{Mutex, Arc};
use config::Config;
use artifactory::{ArtifactoryClient, ArtifactoryError};

pub struct CertificateRegistry {
    certificates: Mutex<HashMap<String, String>>,
    artifactory: ArtifactoryClient,
}

impl CertificateRegistry {
    pub fn new(config: Arc<Config>) -> CertificateRegistry {
        let base_uri = config.artifactory.base_uri.clone();
        let path     = config.artifactory.certificate_path.clone();

        CertificateRegistry {
            certificates: Mutex::new(HashMap::new()),
            artifactory: ArtifactoryClient::new(base_uri, path),
        }
    }

    pub fn fetch<F, T>(&self, application: &str, f: F) -> Result<T, ArtifactoryError>
        where F: FnOnce(&str) -> T {
            let mut certificates = self.certificates.lock().unwrap();

            if let Some(api_key) = certificates.get(application) {
                return Ok(f(api_key))
            }

            let api_key = try!(self.artifactory.fetch_api_key(application));
            certificates.insert(String::from(application), api_key);

            Ok(f(certificates.get(application).unwrap()))
    }
}
