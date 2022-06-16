//use std::time::Duration;
use web_push::*;

use futures::{Future};

use common::events::push_notification::PushNotification;
use common::kafka::{Whoosh, GenericRor};

use std::sync::{Arc,Mutex};
//use serde::de::Unexpected::Option;
//use slog::MutexDrainError::Mutex;
//use tokio::runtime::Runtime;
use std::thread;

use core::option::Option;

pub struct Notifier {
    client: Arc<WebPushClient>,
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier {
            client: Arc::new(WebPushClient::new().unwrap()),
        }
    }

    pub fn notify_helper(
        &self,
        message: WebPushMessage
    ) -> Box<dyn Future<Output=Result<(),Result<Arc<WebPushError>,()>>> + Unpin + Send + 'static> {
        let mute = Arc::new(Mutex::<Option<Option<Option<Arc<WebPushError>>>>>::new(None));
        let ret = Box::new(Whoosh::new(mute.clone()));
        /*let thread = match Runtime::new(){
            Ok(x) => {x}
            Err(_) => {
                *mute.lock().unwrap() = Some(Some(None));
                return ret;
            }
        };*/
        let clien = self.client.clone();
        tokio::spawn(async move{
            let a = match clien.send(message).await {
                Ok(_) => Some(None),
                Err(x) => Some(Some(Arc::new(x.clone())))
            };

            *mute.lock().unwrap() = Some(a)
        });
        return ret;


    }

    pub fn notify(
        &self,
        event: &PushNotification,
        fcm_api_key: Option<&VapidSignature>,
    ) -> Box<dyn Future<Output=Result<(),Result<Arc<WebPushError>,()>>> + Unpin + Send + 'static> {
        if let Ok(message) =  Self::build_message(&event, fcm_api_key) {
            return self.notify_helper(message);
        };
        return Box::new(GenericRor::new(WebPushError::ServerError(None)));
    }

    fn build_message(
        pn: &PushNotification,
        fcm_api_key: Option<&VapidSignature>,
    ) -> Result<WebPushMessage, WebPushError> {
        let web = pn.get_web();

        let subscription_info =
            SubscriptionInfo::new(pn.get_device_token(), web.get_auth(), web.get_p256dh());

        let mut message = WebPushMessageBuilder::new(&subscription_info)?;

        if web.has_payload() {
            message.set_payload(ContentEncoding::Aes128Gcm, web.get_payload().as_bytes());
        }

        if web.has_ttl() {
            message.set_ttl(web.get_ttl() as u32);
        }

        if let Some(key) = fcm_api_key {
            message.set_vapid_signature(key.clone());
        }

        message.build()
    }
}
