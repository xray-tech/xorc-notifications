use web_push::*;
use events::push_notification::PushNotification;
use tokio_core::reactor::Core;
use futures::sync::mpsc::{Sender, Receiver};
use futures::{Future, Stream, Sink};
use metrics::RESPONSE_TIMES_HISTOGRAM;
use std::time::Duration;
use base64;

pub struct Notifier {}

pub type NotifierMessage = (Result<Option<String>, ()>, PushNotification);
pub type ProducerMessage = (PushNotification, Option<Result<(), WebPushError>>);

impl Notifier {
    pub fn new() -> Notifier {
        Notifier {}
    }

    pub fn run(&self, consumer_rx: Receiver<NotifierMessage>, producer_tx: Sender<ProducerMessage>) {
        let mut core = Core::new().unwrap();
        let handle = core.handle();
        let client = WebPushClient::new(&handle).unwrap();

        let sender = consumer_rx.for_each(|(api_functionality, event)| {
            let tx = producer_tx.clone();

            match api_functionality {
                Ok(gcm_api_key) => {
                    match Self::build_message(&event, gcm_api_key) {
                        Ok(message) => {
                            let response_time = RESPONSE_TIMES_HISTOGRAM.start_timer();

                            let work = client.send_with_timeout(message, Duration::from_secs(2)).then(|res| {
                                response_time.observe_duration();
                                tx.send((event, Some(res)))
                            }).then(|_| Ok(()));

                            handle.spawn(work);
                        }
                        Err(e) => {
                            let work = tx.send((event, Some(Err(e)))).then(|_| Ok(()));

                            handle.spawn(work);
                        }
                    }
                },
                Err(_) => {
                    let work = tx.send((event, None)).then(|_| Ok(()));

                    handle.spawn(work);
                }
            }

            Ok(())
        });

        core.run(sender).unwrap();
    }

    fn build_message(pn: &PushNotification, gcm_api_key: Option<String>) -> Result<WebPushMessage, WebPushError> {
        let web = pn.get_web();
        let auth = base64::decode(web.get_auth()).unwrap();
        let p256dh = base64::decode(web.get_p256dh()).unwrap();

        let mut message = WebPushMessageBuilder::new(pn.get_device_token(), &auth, &p256dh)?;

        if web.has_payload() {
            message.set_payload(ContentEncoding::AesGcm, web.get_payload().as_bytes());
        }

        if web.has_ttl() {
            message.set_ttl(web.get_ttl() as u32);
        }

        if let Some(ref key) = gcm_api_key {
            message.set_gcm_key(key);
        }


        message.build()
    }
}
