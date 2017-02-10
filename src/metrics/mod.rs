use prometheus::{CounterVec, Histogram, HistogramVec, Gauge, TextEncoder, Encoder, self};
use std::env;

use hyper::header::ContentType;
use hyper::server::{Server, Request, Response, Listening};
use hyper::mime::Mime;

lazy_static! {
    pub static ref CALLBACKS_COUNTER: CounterVec = register_counter_vec!(
        "push_notifications_total",
        "Total number of push notifications made.",
        &["status"]
    ).unwrap();

    pub static ref CALLBACKS_INFLIGHT: Gauge = register_gauge!(
        "push_notifications_in_flight",
        "Number of push notifications in flight"
    ).unwrap();

    pub static ref RESPONSE_TIMES_HISTOGRAM: Histogram = register_histogram!(
        "http_request_latency_seconds",
        "The HTTP request latencies in seconds"
    ).unwrap();

    pub static ref APNS_CONNECTIONS: Gauge = register_gauge!(
        "apns_http2_connections",
        "Number of http2 connections to Apple push notification service"
    ).unwrap();

    pub static ref POOL_UPDATE: HistogramVec = register_histogram_vec!(
        "apns_pool_update",
        "The time it takes to update the pool in apns consumer",
        &["type"]
    ).unwrap();
}

pub trait StatisticsServer {}

impl StatisticsServer {
    pub fn handle() -> Listening {
        let port = match env::var("PORT") {
            Ok(val) => val,
            Err(_) => String::from("8081"),
        };

        let addr = format!("0.0.0.0:{}", port);

        let server = Server::http(&*addr).unwrap();
        let encoder = TextEncoder::new();

        server.handle(move |_: Request, mut res: Response| {
            let metric_families = prometheus::gather();
            let mut buffer = vec![];
            let content_type = ContentType(encoder.format_type().parse::<Mime>().unwrap());

            encoder.encode(&metric_families, &mut buffer).unwrap();
            res.headers_mut().set(content_type);
            res.send(&buffer).unwrap();
        }).unwrap()
    }
}
