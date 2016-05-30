use apns2::{Provider, APSAlert, Response, Payload, DeviceToken, Notification, NotificationOptions};
use events::apn::Apn;
use std::io::Read;

pub struct Apns2Notifier {
    apns2_provider: Provider,
}

impl Apns2Notifier {
    pub fn new<R: Read>(mut certificate: R, mut private_key: R) -> Apns2Notifier {
        Apns2Notifier {
            apns2_provider: Provider::from_reader(true, &mut certificate, &mut private_key),
        }
    }

    pub fn send(&self, event: &Apn) -> Result<Response, Response> {
        let alert        = APSAlert::Plain(event.get_message().to_string());
        let payload      = Payload::new(alert, 1, "default");
        let token        = DeviceToken::new(event.get_device_token());

        let options = NotificationOptions {
            apns_priority: if event.has_apns_priority() { Some(event.get_apns_priority()) } else { None },
            apns_id: if event.has_apns_id() { Some(event.get_apns_id()) } else { None },
            apns_expiration: if event.has_apns_expiration() { Some(event.get_apns_expiration()) } else { None },
            apns_topic: if event.has_apns_topic() { Some(event.get_apns_topic()) } else { None }, ..Default::default()
        };

        let notification = Notification::new(payload, token, options);

        self.apns2_provider.push(notification)
    }
}
