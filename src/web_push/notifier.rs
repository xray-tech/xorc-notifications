use web_push::*;
use std::time::Duration;

use futures::{
    future::{
        Either,
        err,
    },
    Future,
};

use common::{
    events::push_notification::PushNotification,
};

pub struct Notifier {
    client: WebPushClient
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier {
            client: WebPushClient::new().unwrap(),
        }
    }

    pub fn notify(
        &self,
        event: &PushNotification,
        fcm_api_key: Option<&String>,
    ) -> impl Future<Item=(), Error=WebPushError>
    {
        match Self::build_message(&event, fcm_api_key) {
            Ok(message) => {
                Either::A(self.client.send_with_timeout(message, Duration::from_secs(2)))
            }
            Err(e) => {
                Either::B(err(e))
            }
        }
    }

    fn build_message(pn: &PushNotification, fcm_api_key: Option<&String>) -> Result<WebPushMessage, WebPushError> {
        let web = pn.get_web();

        let subscription_info = SubscriptionInfo::new(
            pn.get_device_token(),
            web.get_auth(),
            web.get_p256dh(),
        );

        let mut message = WebPushMessageBuilder::new(&subscription_info)?;

        if web.has_payload() {
            message.set_payload(ContentEncoding::AesGcm, web.get_payload().as_bytes());
        }

        if web.has_ttl() {
            message.set_ttl(web.get_ttl() as u32);
        }

        if let Some(key) = fcm_api_key {
            message.set_gcm_key(key);
        }


        message.build()
    }
}
