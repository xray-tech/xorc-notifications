use events::push_notification::PushNotification;
use apns2::request::notification::*;
use apns2::request::payload::Payload;
use apns2::error::Error;
use apns2::client::FutureResponse;
use serde_json::{self, Value};
use serde_json::error::Error as JsonError;
use tokio_core::reactor::Handle;
use certificate_registry::ApnsConnectionParameters;
use apns2::client::Client;
use std::io::Cursor;
use std::time::Duration;
use tokio_timer::Timeout;

pub struct Notifier {
    client: Client,
    topic: String,
}

impl Notifier {
    pub fn new(
        handle: &Handle,
        connection_parameters: ApnsConnectionParameters,
        topic: String,
    ) -> Result<Notifier, Error> {
        let client = match connection_parameters {
            ApnsConnectionParameters::Certificate {pkcs12, password, endpoint} =>
                Client::certificate(&mut Cursor::new(&pkcs12), &password, handle, endpoint)?,
            ApnsConnectionParameters::Token {pkcs8, key_id, team_id, endpoint} =>
                Client::token(&mut Cursor::new(&pkcs8), key_id.as_ref(), team_id.as_ref(), handle, endpoint)?,
        };

        Ok(Notifier { client, topic })
    }

    pub fn notify(&self, event: &PushNotification) -> Timeout<FutureResponse> {
        self.client.send_with_timeout(self.gen_payload(event), Duration::from_secs(3))
    }

    fn gen_payload(&self, event: &PushNotification) -> Payload {
        let notification_data = event.get_apple();
        let headers = notification_data.get_headers();

        let mut options = NotificationOptions {
            ..Default::default()
        };

        if headers.has_apns_priority() {
            match headers.get_apns_priority() {
                10 => options.apns_priority = Priority::High,
                _ => options.apns_priority = Priority::Normal,
            }
        }
        if event.has_correlation_id() {
            options.apns_id = Some(String::from(event.get_correlation_id()));
        }
        if headers.has_apns_expiration() {
            options.apns_expiration = Some(headers.get_apns_expiration() as u64);
        }
        if headers.has_apns_topic() {
            options.apns_topic = Some(String::from(headers.get_apns_topic()));
        } else {
            options.apns_topic = Some(self.topic.clone());
        }

        let mut payload = if notification_data.has_localized() {
            let alert_data = notification_data.get_localized();
            let mut builder =
                LocalizedNotificationBuilder::new(alert_data.get_title(), alert_data.get_body());

            if alert_data.has_title_loc_key() {
                builder.set_title_loc_key(alert_data.get_title_loc_key());
            }
            if alert_data.get_title_loc_args().len() > 0 {
                builder.set_title_loc_args(alert_data.get_title_loc_args().to_vec());
            }
            if alert_data.has_action_loc_key() {
                builder.set_action_loc_key(alert_data.get_action_loc_key());
            }
            if alert_data.has_launch_image() {
                builder.set_launch_image(alert_data.get_launch_image());
            }
            if alert_data.has_loc_key() {
                builder.set_loc_key(alert_data.get_loc_key());
            }
            if alert_data.get_loc_args().len() > 0 {
                builder.set_loc_args(alert_data.get_loc_args().to_vec());
            }
            if notification_data.has_badge() {
                builder.set_badge(notification_data.get_badge());
            }
            if notification_data.has_sound() {
                builder.set_sound(notification_data.get_sound());
            }
            if notification_data.has_category() {
                builder.set_category(notification_data.get_category());
            }
            if alert_data.has_mutable_content() && alert_data.get_mutable_content() {
                builder.set_mutable_content();
            }

            builder.build(event.get_device_token(), options)
        } else if notification_data.has_silent() {
            SilentNotificationBuilder::new().build(event.get_device_token(), options)
        } else {
            let mut builder = PlainNotificationBuilder::new(notification_data.get_plain());

            if notification_data.has_badge() {
                builder.set_badge(notification_data.get_badge());
            }
            if notification_data.has_sound() {
                builder.set_sound(notification_data.get_sound());
            }
            if notification_data.has_category() {
                builder.set_category(notification_data.get_category());
            }

            builder.build(event.get_device_token(), options)
        };

        if notification_data.has_custom_data() {
            let custom_data = notification_data.get_custom_data();

            let v: Result<Value, JsonError> = serde_json::from_str(custom_data.get_body());
            match v {
                Ok(json) => {
                    if let Err(e) = payload.add_custom_data(custom_data.get_key(), &json) {
                        error!("Couldn't serialize custom data {:?}", e);
                    };
                }
                Err(e) => {
                    error!("Non-json custom data: {:?}", e);
                }
            }
        }

        payload
    }
}
