use common::{
    events::{
        application::Application,
        push_notification::PushNotification,
        http_request::HttpRequest,
    },
    kafka::EventHandler,
    metrics::*
};

use futures::{Future, future::ok};
use requester::Requester;
use producer::HttpResponseProducer;

pub struct HttpRequestHandler {
    producer: HttpResponseProducer,
    requester: Requester,
}

impl HttpRequestHandler {
    pub fn new() -> HttpRequestHandler {
        let producer = HttpResponseProducer::new();
        let requester = Requester::new();

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
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
        warn!("We don't handle push notification events here");
        Box::new(ok(()))
    }

    fn handle_http(
        &self,
        key: Option<Vec<u8>>,
        event: HttpRequest,
    ) -> Box<Future<Item = (), Error = ()> + 'static + Send> {
        let producer = self.producer.clone();

        let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();
        CALLBACKS_INFLIGHT.inc();

        let request_send = self.requester.request(&event)
            .then(move |response| {
                timer.observe_duration();
                CALLBACKS_INFLIGHT.dec();
                producer.respond(key, event, response)
            })
            .then(|_| ok(()));

        Box::new(request_send)
    }

    fn handle_config(&self, _: &str, _: Option<Application>) {
        debug!("Skipping configuration");
    }
}
