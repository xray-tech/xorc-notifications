pub mod sender;
pub mod instruments;

#[macro_use]
mod macros;

use std::sync::Arc;
use metrics::instruments::{Gauge, Counter, Timer};

register_counters!(successful, failure, certificate_missing);
register_timers!(response_time);
register_gauges!(in_flight);

pub struct Metrics<'a> {
    pub counters: Counters<'a>,
    pub timers: Timers<'a>,
    pub gauges: Gauges<'a>,
}

impl<'a> Metrics<'a> {
    pub fn new() -> Arc<Metrics<'a>> {
        Arc::new(Metrics {
            gauges: Gauges::new(),
            counters: Counters::new(),
            timers: Timers::new(),
        })
    }
}
