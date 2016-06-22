pub mod sender;
pub mod instruments;

#[macro_use]
mod macros;

use std::sync::Arc;
use metrics::instruments::{Counter, Timer};

register_counters!(successful, failure, certificate_missing);
register_timers!(response_time);

pub struct Metrics<'a> {
    pub counters: Counters<'a>,
    pub timers: Timers<'a>,
}

impl<'a> Metrics<'a> {
    pub fn new() -> Arc<Metrics<'a>> {
        Arc::new(Metrics {
            counters: Counters::new(),
            timers: Timers::new(),
        })
    }
}
