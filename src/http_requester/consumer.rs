use common::{
    events::{
        application::Application,
        push_notification::PushNotification,
        http_request::HttpRequest,
    },
    kafka::EventHandler,
    metrics::*
};

use futures::{Future, future::{ok, err}, FutureExt};
use crate::requester::{Requester};
use crate::producer::HttpResponseProducer;
use std::sync::{Arc, Mutex};
use std::thread;
use common::kafka::FutureMime;

pub struct HttpRequestHandler {
    producer: Arc<HttpResponseProducer>,
    requester: Arc<Requester>,
}

impl HttpRequestHandler {
    pub fn new() -> HttpRequestHandler {
        let producer = Arc::new(HttpResponseProducer::new());
        let requester = Arc::new(Requester::new());

        HttpRequestHandler {
            producer,
            requester,
        }
    }
}

impl EventHandler for HttpRequestHandler {
    fn accepts(&self, _: &PushNotification) -> bool { false }

    fn handle_notification(
        &self,
        _: Option<Vec<u8>>,
        _: PushNotification,
    ) -> Box<dyn Future<Output=Result<(),()>> + 'static + Send + Unpin> {
        warn!("We don't handle push notification events here");
        Box::new(err(()))
    }

    fn handle_http(
        &self,
        key: Option<Vec<u8>>,
        event: HttpRequest,
    ) -> Box<dyn Future<Output=Result<(),()>> + Unpin + Send> {
        let producer = self.producer.clone();

        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
        CALLBACKS_INFLIGHT.inc();

        Box::new({
            let mute = Arc::new(Mutex::<Option<Result<(),()>>>::new(None));
            let ret = mute.clone();
            let req = self.requester.clone();
            thread::spawn(|| async move {
                let b = event.clone();
                let a =req.request(&event)
                    .then(move |response| {
                        timer.observe_duration();
                        CALLBACKS_INFLIGHT.dec();
                        producer.respond(key, b, response)
                    })
                    .then(|_| ok(())).await;
                *mute.lock().unwrap() = Some(a);
            });
            FutureMime::new(ret)
        })

    }

    fn handle_config(&self, _: &str, _: Option<Application>) {
        debug!("Skipping configuration");
    }
}
