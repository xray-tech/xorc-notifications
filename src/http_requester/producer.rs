use common::{
    events::{
        http_request::HttpRequest,
        http_response::*,
        rpc::{Response},
    },
    kafka::{
        DeliveryFuture,
        ResponseProducer
    },
    metrics::*
};
use std::{collections::HashMap, str};
use requester::{HttpResult, RequestError};

use CONFIG;

pub struct HttpResponseProducer {
    producer: ResponseProducer,
}

impl HttpResponseProducer {
    pub fn new() -> HttpResponseProducer {
        HttpResponseProducer {
            producer: ResponseProducer::new(&CONFIG.kafka)
        }
    }

    pub fn respond(
        &self,
        key: Option<Vec<u8>>,
        mut event: HttpRequest,
        result: Result<HttpResult, RequestError>
    ) -> DeliveryFuture
    {
        let mut header = Response::new();
        header.set_field_type("http.HttpResponse".to_string());
        header.set_request(event.take_header());

        let mut response = HttpResponse::new();
        response.set_header(header);

        match result {
            Ok(http_result) => {
                CALLBACKS_COUNTER.with_label_values(&[http_result.code.as_str()]).inc();

                let mut payload = HttpResponse_Payload::new();

                let body_vec = http_result.body.to_vec();
                let body = str::from_utf8(&body_vec).unwrap_or("<BYTES>");

                payload.set_response_body(body.to_string());
                payload.set_status_code(http_result.code.as_u16() as i32);

                let headers = http_result
                    .headers
                    .into_iter()
                    .fold(HashMap::new(), |mut acc, (key, value)| {
                        if let Some(key) = key {
                            acc.insert(
                                String::from(key.as_str()),
                                String::from(value.to_str().unwrap_or("")),
                            );
                        };

                        acc
                    });

                payload.set_headers(headers);
                response.set_payload(payload);

                info!(
                    "Successful HTTP request";
                    "request" => &event,
                    "response" => &response
                );
            }
            Err(RequestError::Timeout) => {
                CALLBACKS_COUNTER.with_label_values(&["timeout"]).inc();
                response.set_connection_error(HttpResponse_SocketError::Timeout);

                error!(
                    "HTTP request timeout";
                    "request" => &event,
                    "response" => &response
                );
            }
            Err(RequestError::Connection) => {
                CALLBACKS_COUNTER.with_label_values(&["connection"]).inc();
                response.set_connection_error(HttpResponse_SocketError::ConnectionError);

                error!(
                    "HTTP request connection error";
                    "request" => &event,
                    "response" => &response
                );
            }
        }

        self.producer.publish(key, &response)
    }
}

impl Clone for HttpResponseProducer {
    fn clone(&self) -> Self {
        Self {
            producer: self.producer.clone(),
        }
    }
}
