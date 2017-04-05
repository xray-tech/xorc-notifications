use prometheus::{CounterVec, Histogram, Gauge, TextEncoder, Encoder, self};
use std::env;
use futures::future::{ok, FutureResult};

use hyper::header::ContentType;
use hyper::header::ContentLength;
use hyper::server::{Http, Service, Request, Response};
use hyper::mime::Mime;
use hyper::Error as HyperError;
use futures::sync::oneshot::Receiver;
use futures::Future;

lazy_static! {
    pub static ref CALLBACKS_COUNTER: CounterVec = register_counter_vec!(
        "push_notifications_total",
        "Total number of push notifications made.",
        &["status"]
    ).unwrap();

    pub static ref RESPONSE_TIMES_HISTOGRAM: Histogram = register_histogram!(
        "http_request_latency_seconds",
        "The HTTP request latencies in seconds"
    ).unwrap();

    pub static ref CALLBACKS_INFLIGHT: Gauge = register_gauge!(
        "push_notifications_in_flight",
        "Number of push notifications in flight"
    ).unwrap();

}

#[derive(Clone,Copy)]
pub struct StatisticsServer;

impl Service for StatisticsServer {
    type Request = Request;
    type Response = Response;
    type Error = HyperError;
    type Future = FutureResult<Response, HyperError>;

    fn call(&self, _: Request) -> Self::Future {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];

        encoder.encode(&metric_families, &mut buffer).unwrap();

        let content_type = ContentType(encoder.format_type().parse::<Mime>().unwrap());
        let content_length = ContentLength(buffer.len() as u64);

        ok({
            Response::new()
                .with_header(content_length)
                .with_header(content_type)
                .with_body(buffer)
        })
    }
}

impl StatisticsServer {
    pub fn handle(rx: Receiver<()>) {
        let port = match env::var("PORT") {
            Ok(val) => val,
            Err(_) => String::from("8081"),
        };

        let addr = format!("0.0.0.0:{}", port).parse().unwrap();
        let server = Http::new().bind(&addr, || Ok(StatisticsServer)).unwrap();

        server.run_until(rx.map_err(|_| ())).unwrap();
    }
}
