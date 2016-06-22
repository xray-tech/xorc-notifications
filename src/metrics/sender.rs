use std::sync::atomic::{Ordering, AtomicBool};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use influent::measurement::{Measurement, Value};
use influent::client::{Client, Credentials};
use influent::create_client;
use config::Config;
use std::env;
use metrics::Metrics;

pub struct MetricsSender<'a> {
    pub metrics: Arc<Metrics<'a>>,
    pub control: Arc<AtomicBool>,
    pub config: Arc<Config>,
}

impl<'a> MetricsSender<'a> {
    pub fn run(&self) {
        let hostname = match env::var("MESOS_TASK_ID") {
            Ok(val) => val,
            Err(_) => String::from("undefined")
        };

        let influx = create_client(Credentials {
            username: &self.config.metrics.login,
            password: &self.config.metrics.password,
            database: &self.config.metrics.database,
        }, vec![&self.config.metrics.uri]);

        let tick_duration = Duration::new(self.config.metrics.tick_duration, 0);
        let counter_name  = format!("{}-counters", self.config.metrics.application);
        let timer_name    = format!("{}-timers", self.config.metrics.application);

        while self.control.load(Ordering::Relaxed) {
            thread::park_timeout(tick_duration);

            let mut measurements: Vec<Measurement> = self.metrics.counters.as_vec().iter().map(|counter| {
                (counter.name, counter.collect() as i64)
            }).map(|counter| {
                let mut measurement = Measurement::new(&counter_name);
                measurement.add_tag("metric", counter.0);
                measurement.add_tag("hostname", &hostname);
                measurement.add_field("value", Value::Integer(counter.1));

                measurement
            }).collect();

            for timer_result in self.metrics.timers.as_vec().iter().map(|v| v.collect()) {
                if let Ok(timer) = timer_result {
                    let mut measurement = Measurement::new(&timer_name);
                    measurement.add_tag("metric", timer.name);
                    measurement.add_tag("hostname", &hostname);
                    measurement.add_field("min", Value::Integer(timer.min as i64));
                    measurement.add_field("mean", Value::Integer(timer.mean as i64));
                    measurement.add_field("p70", Value::Integer(timer.p70 as i64));
                    measurement.add_field("p90", Value::Integer(timer.p90 as i64));
                    measurement.add_field("p95", Value::Integer(timer.p95 as i64));
                    measurement.add_field("p99", Value::Integer(timer.p99 as i64));
                    measurement.add_field("max", Value::Integer(timer.max as i64));

                    measurements.push(measurement);
                };
            }


            let _ = influx.write_many(measurements, None);
        }
    }
}


