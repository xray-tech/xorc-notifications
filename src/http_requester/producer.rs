use common::{
    events::{
        http_request::HttpRequest,
        http_response::HttpResponse,
        map_field_entry::MapFieldEntry,
    },
    kafka::{
        DeliveryFuture,
        ResponseProducer
    },
    metrics::*
};
use protobuf::RepeatedField;
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
        event: HttpRequest,
        result: Result<HttpResult, RequestError>
    ) -> DeliveryFuture
    {
        let mut response = HttpResponse::new();
        response.set_request(event);

        match result {
            Ok(http_result) => {
                CALLBACKS_COUNTER.with_label_values(&[http_result.code.as_str()]).inc();

                response.set_response_body(http_result.body.to_vec());
                response.set_status_code(http_result.code.as_u16() as i32);

                let headers = http_result
                    .headers
                    .into_iter()
                    .fold(Vec::new(), |mut acc, (key, value)| {
                        if let Some(key) = key {
                            let mut mfe = MapFieldEntry::new();
                            mfe.set_key(String::from(key.as_str()));
                            mfe.set_value(String::from(value.to_str().unwrap_or("")));
                            acc.push(mfe);
                        };

                        acc
                    });

                response.set_headers(RepeatedField::from_vec(headers));
                info!("Successful HTTP request"; &response);
            }
            Err(RequestError::Timeout) => {
                CALLBACKS_COUNTER.with_label_values(&["timeout"]).inc();
                response.set_response_body("Timeout".as_bytes().to_vec());
                error!("HTTP request timeout"; &response);
            }
            Err(RequestError::Connection) => {
                CALLBACKS_COUNTER.with_label_values(&["connection"]).inc();
                response.set_response_body("Connection Error".as_bytes().to_vec());
                error!("HTTP request connection error"; &response);
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
