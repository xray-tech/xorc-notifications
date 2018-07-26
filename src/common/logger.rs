use std::{env, io};
use slog::{self, Drain, KV, Record, Serializer};
use slog_term::{TermDecorator, CompactFormat};
use slog_async::Async;
use slog_json::Json;

use events::{
    push_notification::PushNotification,
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

        let environment = env::var("RUST_ENV").unwrap_or_else(|_| String::from("development"));

        slog::Logger::root(
            drain,
            o!("application_name" => application_name, "environment" => environment)
        )
    }
}

impl KV for PushNotification {
    fn serialize(&self, _record: &Record, serializer: &mut Serializer) -> slog::Result {
        serializer.emit_str("action", "push_result")?;
        serializer.emit_str("correlation_id", self.get_correlation_id())?;
        serializer.emit_str("device_token", self.get_device_token())?;
        serializer.emit_str("app_id", self.get_application_id())?;
        serializer.emit_str("campaign_id", self.get_campaign_id())?;
        serializer.emit_str("event_source", self.get_header().get_source())?;

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
