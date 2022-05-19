use prometheus::{self, CounterVec, Encoder, Gauge, Histogram, TextEncoder};
use std::env;

use hyper::{
    Body,
    Error,
    Request,
    Response,
    server::Server,
    service::{make_service_fn, service_fn},
    header};

use tokio::runtime::Runtime;

use futures::{channel::oneshot::Receiver};

lazy_static! {
    pub static ref CALLBACKS_COUNTER: CounterVec = register_counter_vec!(
        "push_notifications_total",
        "Total number of push notifications responded.",
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
    pub static ref TOKEN_CONSUMERS: Gauge = register_gauge!(
        "apns_token_consumers",
        "Number of token-based consumers to Apple push notification service"
    ).unwrap();
    pub static ref CERTIFICATE_CONSUMERS: Gauge = register_gauge!(
        "apns_certificate_consumers",
        "Number of certificate-based consumers to Apple push notification service"
    ).unwrap();
    pub static ref NUMBER_OF_APPLICATIONS: Gauge = register_gauge!(
        "push_notications_number_of_applications",
        "Number of applications sending push notifications"
    ).unwrap();
}

#[derive(Clone, Copy)]
pub struct StatisticsServer;

impl StatisticsServer {
    fn prometheus(_: Request<Body>) -> Response<Body> {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut buffer = vec![];
        let builder = Response::builder();

        encoder.encode(&metric_families, &mut buffer).unwrap();

        builder
            .header(header::CONTENT_TYPE, encoder.format_type())
            .body(buffer.into()).unwrap()
    }

    pub fn handle(_rx: Receiver<()>) {
        let port = match env::var("PORT") {
            Ok(val) => val,
            Err(_) => String::from("8081"),
        };

        let addr = format!("0.0.0.0:{}", port).parse().unwrap();

        let make_svc = make_service_fn(|_| async {
            Ok::<_, Error>(service_fn(|req| async {
                Ok::<_, Error>(Self::prometheus(req))
            }))
        });

        let server = Server::bind(&addr)
            .serve(make_svc);
            //.map_err(|e| eprintln!("server error: {}", e));

        match Runtime::new(){
            Ok(x) => {x.spawn(server/*.select2(rx).then(move |_| Ok(()))*/);}
            Err(_) => {println!("WOOPS! HANDLE FAILED!")}
        };
    }
}
