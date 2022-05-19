use std::{env, io};
use slog::{self, Drain, Record, Serializer, KV, Key};
use slog_term::{TermDecorator, CompactFormat};
use slog_async::Async;
use slog_json::Json;

use crate::events::{
    push_notification::PushNotification,
    http_response::HttpResponse,
    http_request::HttpRequest,
    application::Application
};

#[derive(Debug)]
pub enum LogAction {
    ConsumerCreate,
    ConsumerDelete,
    NotificationResult,
}

pub struct Logger;

impl Logger {
    /// Builds a new logger. Depending on `LOG_FORMAT` environment variable,
    /// either produces colorful text or JSON.
    pub fn build(application_name: &'static str) -> slog::Logger {
        let drain = match env::var("LOG_FORMAT") {
            Ok(ref val) if val == "json" => {
                let drain = Json::new(io::stdout()).add_default_keys().build().fuse();
                Async::new(drain).build().fuse()
            }
            _ => {
                let decorator = TermDecorator::new().stdout().build();
                let drain = CompactFormat::new(decorator).build().fuse();
                Async::new(drain).build().fuse()
            }
        };

        let environment = env::var("RUST_ENV")
            .unwrap_or_else(|_| String::from("development"));

        slog::Logger::root(
            drain,
            o!("application_name" => application_name, "environment" => environment)
        )
    }
}

impl KV for PushNotification {
    fn serialize(&self, _record: &Record, serializer: &mut dyn Serializer) -> slog::Result {
        serializer.emit_str("device_token", self.get_device_token())?;
        serializer.emit_str("universe", self.get_universe())?;
        serializer.emit_str("correlation_id", self.get_header().get_correlation_id())?;

        Ok(())
    }
}

impl slog::Value for HttpResponse {
    fn serialize(&self, _record: &Record, _key: Key, serializer: &mut dyn Serializer) -> slog::Result {
        if self.has_payload() {
            serializer.emit_str(
                "status_code",
                &self.get_payload().get_status_code().to_string(),
            )
        } else {
            serializer.emit_str(
                "status_code",
                &format!("{:?}", self.get_connection_error()),
            )
        }
    }
}

impl slog::Value for HttpRequest {
    fn serialize(&self, _record: &Record, _key: Key, serializer: &mut dyn Serializer) -> slog::Result {
        serializer.emit_str("correlation_id", self.get_header().get_correlation_id())?;
        serializer.emit_str("request_type", self.get_request_type().as_ref())?;
        serializer.emit_str("request_body", self.get_body())?;
        serializer.emit_str("timeout", &self.get_timeout().to_string())?;

        let mut curl = format!("curl -X {} ", self.get_request_type().as_ref());

        if self.has_body() {
            curl.push_str("--data \"");
            curl.push_str(self.get_body().replace("\"", "\\\"").as_ref());
            curl.push_str("\" ");
        }

        for (key, value) in self.get_headers().iter() {
            curl.push_str("-H \"");
            curl.push_str(key);
            curl.push_str(": ");
            curl.push_str(value);
            curl.push_str("\" ");
        }

        curl.push_str(self.get_uri());

        if !self.get_params().is_empty() {
            curl.push_str("?");

            for (key, value) in self.get_params().iter() {
                curl.push_str(key);
                curl.push_str("=");
                curl.push_str(value)
            }
        }

        serializer.emit_str("curl", curl.as_ref())?;

        Ok(())
    }
}

impl KV for Application {
    fn serialize(&self, _record: &Record, serializer: &mut dyn Serializer) -> slog::Result {
        serializer.emit_str("app_id", self.get_id())?;

        if self.has_organization() {
            serializer.emit_str("organization", self.get_organization())?;
        }

        Ok(())
    }
}
