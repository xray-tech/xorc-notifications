use std::sync::atomic::{AtomicUsize, Ordering};
use histogram::Histogram;
use std::sync::Mutex;
use time::precise_time_ns;

pub struct Gauge<'a> {
    pub name: &'a str,
    value: AtomicUsize,
}

impl<'a> Gauge<'a> {
    pub fn new(name: &'a str) -> Gauge<'a> {
        Gauge {
            name: name,
            value: AtomicUsize::new(0),
        }
    }

    pub fn increment(&self, value: usize) {
        self.value.fetch_add(value, Ordering::SeqCst);
    }

    pub fn decrement(&self, value: usize) {
        self.value.fetch_sub(value, Ordering::SeqCst);
    }

    pub fn collect(&self) -> usize {
        self.value.load(Ordering::SeqCst)
    }
}

pub struct Counter<'a> {
    pub name: &'a str,
    value: AtomicUsize,
}

impl<'a> Counter<'a> {
    pub fn new(name: &'a str) -> Counter<'a> {
        Counter {
            name: name,
            value: AtomicUsize::new(0)
        }
    }

    pub fn increment(&self, value: usize) {
        self.value.fetch_add(value, Ordering::Relaxed);
    }

    pub fn collect(&self) -> usize {
        self.value.load(Ordering::Relaxed)
    }
}

pub struct Timer<'a> {
    pub name: &'a str,
    reservoir: Mutex<Histogram>,
}

#[derive(Debug)]
pub struct TimerSnapshot<'a> {
    pub name: &'a str,
    pub min: u64,
    pub mean: u64,
    pub p70: u64,
    pub p90: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

impl<'a> Timer<'a> {
    pub fn new(name: &'a str) -> Timer<'a> {
        Timer {
            name: name,
            reservoir: Mutex::new(Histogram::new().unwrap()),
        }
    }

    #[allow(dead_code)]
    pub fn time<A, F>(&self, f: F) -> A where F: FnOnce() -> A {
        let before = precise_time_ns();
        let res = f();
        self.record((precise_time_ns() - before) as u64);
        res
    }

    pub fn record(&self, value: u64) {
        let mut reservoir = self.reservoir.lock().unwrap();
        let _ = reservoir.increment(value);
    }

    pub fn collect(&self) -> Result<TimerSnapshot<'a>, &'static str> {
        let mut reservoir = self.reservoir.lock().unwrap();

        let result = TimerSnapshot {
            name: self.name,
            min: try!(reservoir.minimum()),
            mean: try!(reservoir.mean()),
            p70: try!(reservoir.percentile(70.0)),
            p90: try!(reservoir.percentile(90.0)),
            p95: try!(reservoir.percentile(95.0)),
            p99: try!(reservoir.percentile(99.0)),
            max: try!(reservoir.maximum()),
        };

        let _ = reservoir.clear();

        Ok(result)
    }
}
