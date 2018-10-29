use hyper::{
    Request,
    StatusCode,
    Body,
    client::{Client, HttpConnector},
};
use futures::{
    Future,
    stream::Stream,
    future::err,
};
use std::{
    collections::HashMap,
    time::Duration,
};
use common::events::http_request::HttpRequest;
use http::HeaderMap;
use hyper_tls::HttpsConnector;
use bytes::Bytes;
use tokio_timer::Timer;

pub struct Requester {
    client: Client<HttpsConnector<HttpConnector>>,
    timer: Timer,
}

#[derive(Debug)]
pub enum RequestError {
    Timeout,
    Connection,
}

pub struct HttpResult {
    pub code: StatusCode,
    pub body: Bytes,
    pub headers: HeaderMap
}

impl Requester {
    pub fn new() -> Requester {
        let mut client = Client::builder();
        client.keep_alive(true);

        Requester {
            client: client.build(HttpsConnector::new(4).unwrap()),
            timer: Timer::default(),
        }
    }

    pub fn request(
        &self,
        event: &HttpRequest,
    ) -> impl Future<Item=HttpResult, Error=RequestError> + 'static + Send
    {
        let mut builder = Request::builder();
        builder.method(event.get_request_type().as_ref());

        for (k, v) in event.get_headers().iter() {
            builder.header(k.as_bytes(), v.as_bytes());
        }

        if event.get_params().is_empty() {
            builder.uri(event.get_uri());
        } else {
            builder.uri(Self::uri_with_params(
                event.get_uri(),
                event.get_params(),
            ));
        };

        let request: Request<Body> =
            builder.body(Body::from(event.get_body().as_bytes().to_vec())).unwrap();

        let timeout = self.timer.sleep(Duration::from_millis(event.get_timeout()))
            .then(|_| err(RequestError::Timeout));

        self.client
            .request(request)
            .map_err(|_| { RequestError::Connection })
            .select(timeout)
            .map_err(|_| { RequestError::Timeout })
            .and_then(move |(response, _)| {
                let (parts, body) = response.into_parts();

                body
                    .concat2()
                    .map_err(|_| { RequestError::Connection })
                    .map(move |chunk| {
                        HttpResult {
                            code: parts.status,
                            body: chunk.into_bytes(),
                            headers: parts.headers
                        }
                    })
            })
    }

    fn uri_with_params(uri: &str, params: &HashMap<String, String>) -> String {
        params.iter().fold(format!("{}?", uri), |mut acc, (key, value)| {
            acc.push_str(key.as_ref());
            acc.push_str("=");
            acc.push_str(value.as_ref());
            acc
        })
    }
}
