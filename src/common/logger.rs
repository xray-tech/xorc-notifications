use std::{env, io, collections::HashMap};
use slog::{self, Drain, KV, Record, Serializer};
use slog_term::{TermDecorator, CompactFormat};
use slog_async::Async;
use slog_json::Json;

use events::{
    push_notification::PushNotification,
    http_response::HttpResponse,
    crm::Application
};

#[derive(Debug)]
pub enum LogAction {
    ConsumerCreate,
    ConsumerDelete,
    NotificationResult,
}

pub struct Logger;

impl Logger {
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

#[derive(Clone, Serialize, SerdeValue)]
#[serde(untagged)]
enum Context {
    Map(HashMap<String, String>)
}

impl KV for PushNotification {
    fn serialize(&self, _record: &Record, serializer: &mut Serializer) -> slog::Result {
        serializer.emit_str("device_token", self.get_device_token())?;
        serializer.emit_str("event_source", self.get_header().get_source())?;
        serializer.emit_str("universe", self.get_universe())?;
        serializer.emit_str("correlation_id", self.get_correlation_id())?;
        serializer.emit_serde("context", &Context::Map(self.get_context().clone()))?;

        Ok(())
    }
}

impl KV for HttpResponse {
    fn serialize(&self, _record: &Record, serializer: &mut Serializer) -> slog::Result {
        let request = self.get_request();
        serializer.emit_str("event_source", request.get_header().get_source())?;
        serializer.emit_str("request_type", request.get_request_type().as_ref())?;
        serializer.emit_str("request_body", request.get_body())?;
        serializer.emit_str("status_code", &format!("{}", self.get_status_code()))?;
        serializer.emit_serde("context", &Context::Map(request.get_context().clone()))?;

        let mut curl = format!("curl -X {} ", request.get_request_type().as_ref());

        if self.get_request().has_body() {
            curl.push_str("--data \"");
            curl.push_str(request.get_body().replace("\"", "\\\"").as_ref());
            curl.push_str("\" ");
        }

        for (key, value) in request.get_headers().iter() {
            curl.push_str("-H \"");
            curl.push_str(key);
            curl.push_str(": ");
            curl.push_str(value);
            curl.push_str("\" ");
        }

        curl.push_str(request.get_uri());

        if !request.get_params().is_empty() {
            curl.push_str("?");

            for (key, value) in request.get_params().iter() {
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
    fn serialize(&self, _record: &Record, serializer: &mut Serializer) -> slog::Result {
        serializer.emit_str("app_id", self.get_id())?;

        if self.has_organization() {
            serializer.emit_str("organization", self.get_organization())?;
        }

        Ok(())
    }
}
