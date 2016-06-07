use apns2::*;
use events::push_notification::PushNotification;
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

    pub fn send(&self, event: &PushNotification) -> AsyncResponse {
        let alert   = APSAlert::Plain(event.get_message().to_string());
        let payload = Payload::new(alert, 1, "default");
        let token   = DeviceToken::new(event.get_device_token());
        let headers = event.get_apns_headers();

        let options = NotificationOptions {
            apns_priority: if headers.has_apns_priority() { Some(headers.get_apns_priority()) } else { None },
            apns_id: if event.has_correlation_id() { Some(event.get_correlation_id()) } else { None },
            apns_expiration: if headers.has_apns_expiration() { Some(headers.get_apns_expiration()) } else { None },
            apns_topic: if headers.has_apns_topic() { Some(headers.get_apns_topic()) } else { None }, ..Default::default()
        };

        let notification = Notification::new(payload, token, options);

        self.apns2_provider.push(notification)
    }
}
