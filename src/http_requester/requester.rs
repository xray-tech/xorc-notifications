use hyper::{
    Request,
    StatusCode,
    Body,
    client::{Client, HttpConnector},
};
use std::{
    collections::HashMap,
    time::Duration,
};
use common::events::http_request::HttpRequest;
use hyper::HeaderMap;
use hyper_tls::HttpsConnector;
use bytes::Bytes;
use hyper::body::HttpBody;


pub struct Requester {
    client: Client<HttpsConnector<HttpConnector>>,
    //timer: Timer<None>,
}

#[derive(Debug)]
pub enum RequestError {
    Timeout,
    Connection,
    Thread,
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
            client: client.build(HttpsConnector::new()/*.unwrap()*/),
            //timer: Timer::default(),
        }
    }

    pub async fn request(
        &self,
        event: &HttpRequest,
    ) -> Result<HttpResult,RequestError>
    {
        let mut builder = Request::builder()
            .method(event.get_request_type().as_ref());

        for (k, v) in event.get_headers().iter() {
            builder = builder.header(k.as_bytes(), v.as_bytes());
        }

        if event.get_params().is_empty() {
            builder = builder.uri(event.get_uri());
        } else {
            builder = builder.uri(Self::uri_with_params(
                event.get_uri(),
                event.get_params(),
            ));
        };

        let request: Request<Body> = builder.body(Body::from(event.get_body().as_bytes().to_vec())).unwrap();

        let timeout = tokio::time::timeout(
            Duration::from_millis(event.get_timeout()),
            self.client.request(request));


        return match timeout.await {
            Ok(x)=>{
                match x {
                    Ok(t)=>{
                        let (a,mut b) = t.into_parts();

                        let bod = b.data().await;

                        let by = match bod {
                            Some(x)=> {
                                match x {
                                    Ok(x) => x,
                                    Err(_) => {return Err(RequestError::Thread);}
                                }
                            }
                            None => {return Err(RequestError::Thread);}
                        };


                        Ok(HttpResult{
                            code:a.status,
                            body: by,
                            headers:a.headers
                        })
                    }
                    Err(_) => {
                        return Err(RequestError::Connection)
                    }
                }
            }
            Err(_) =>{
                return Err(RequestError::Timeout);
            }
            /*Either::Left((value1, _)) => {
                match value1 {
                    Ok(x) => {
                        let (parts, body) = x.into_parts();
                        Ok(HttpResult {
                            code: parts.status,
                            body: body.try_into(),
                            headers: parts.headers
                        })
                    }
                    Err(_) => Err(RequestError::Connection)
                }
            }
            Either::Right((value2, _)) => Err(RequestError::Timeout),*/
        }




            /*.select(timeout)
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
            })*/
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
