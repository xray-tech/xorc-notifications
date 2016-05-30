use log::{Log, LogRecord, LogLevel, LogMetadata};
use syslog::{Logger, Severity};
use std::sync::Mutex;
use log::LogLevelFilter;
use std::env;

pub struct SyslogLogger {
    pub writer: Mutex<Box<Logger>>,
}

impl SyslogLogger {
    pub fn new(writer: Box<Logger>) -> SyslogLogger {
        SyslogLogger {
            writer: Mutex::new(writer),
        }
    }

    pub fn get_log_level_filter() -> LogLevelFilter {
        if let Ok(val) = env::var("RUST_LOG") {
            match val.as_ref() {
                "info"  => LogLevelFilter::Info,
                "debug" => LogLevelFilter::Debug,
                "warn"  => LogLevelFilter::Warn,
                "error" => LogLevelFilter::Error,
                _       => LogLevelFilter::Info
            }
        } else {
            LogLevelFilter::Info
        }
    }
}

impl Log for SyslogLogger {
    fn enabled(&self, _metadata: &LogMetadata) -> bool {
        true
    }

    fn log(&self, record: &LogRecord) {
        let metadata = record.metadata();

        let severity = match metadata.level() {
            LogLevel::Error => Severity::LOG_ERR,
            LogLevel::Warn => Severity::LOG_WARNING,
            LogLevel::Info => Severity::LOG_INFO,
            LogLevel::Debug => Severity::LOG_DEBUG,
            LogLevel::Trace => Severity::LOG_DEBUG
        };

        if self.enabled(metadata) {
            let writer = self.writer.lock().unwrap();
            let _ = writer.send_3164(severity, &format!("{}", record.args()));
        }
    }
}
