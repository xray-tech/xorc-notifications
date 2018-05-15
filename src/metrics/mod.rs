use hyper::{
    Body, Request, Response, Server,
    service::service_fn_ok,
    rt::Future,
};
use prometheus::{
    self,
    CounterVec,
    Histogram,
    Gauge,
    TextEncoder,
    Encoder,
};
use futures::{
    sync::oneshot::Receiver,
};
use std::{
    env,
    net::ToSocketAddrs,
};

use tokio;
use http::header;


lazy_static! {
    pub static ref CALLBACKS_COUNTER: CounterVec = register_counter_vec!(
        "push_notifications_total",
        "Total number of push notifications made.",
        &["status"]
    ).unwrap();

    pub static ref REQUEST_COUNTER: CounterVec = register_counter_vec!(
        "push_notifications_requested",
        "Total number of push notification requests made.",
        &["status", "app_id", "campaign_id"]
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

impl StatisticsServer {
    fn service(_: Request<Body>) -> Response<Body> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];

        encoder.encode(&metric_families, &mut buffer).unwrap();

        let mut builder = Response::builder();

        builder.header(
            header::CONTENT_TYPE,
            encoder.format_type()
        );

        builder.body(buffer.into()).unwrap()
    }

    pub fn handle(rx: Receiver<()>) {
        let port = match env::var("PORT") {
            Ok(val) => val,
            Err(_) => String::from("8081"),
        };
        let mut addr_iter = format!("0.0.0.0:{}", port).to_socket_addrs().unwrap();
        let addr = addr_iter.next().unwrap();

        let server = Server::bind(&addr)
            .serve(|| service_fn_ok(Self::service))
            .map_err(|e| println!("server error: {}", e));

        tokio::run(server.select2(rx).then(move |_| Ok(())))
    }
}
