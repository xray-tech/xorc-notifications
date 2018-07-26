use common::events::push_notification::PushNotification;
use common::metrics::*;
use serde_json::error::Error as JsonError;
use serde_json::{self, Value};
use std::io::Read;
use std::time::Duration;
use tokio_timer::Timeout;

use a2::{client::{Client, Endpoint, FutureResponse}, error::Error,
         request::{notification::*, payload::Payload}};

enum NotifierType {
    Token,
    Certificate,
}

pub struct Notifier {
    client: Client,
    topic: String,
    notifier_type: NotifierType,
}

impl Drop for Notifier {
    fn drop(&mut self) {
        match self.notifier_type {
            NotifierType::Token => {
                TOKEN_CONSUMERS.dec();
            }
            NotifierType::Certificate => {
                CERTIFICATE_CONSUMERS.dec();
            }
        }
    }
}

impl Notifier {
    pub fn certificate<R>(
        pkcs12: &mut R,
        password: &str,
        endpoint: Endpoint,
        topic: &str,
    ) -> Result<Notifier, Error>
    where
        R: Read,
    {
        let client = Client::certificate(pkcs12, password, endpoint)?;
        let notifier_type = NotifierType::Certificate;
        CERTIFICATE_CONSUMERS.inc();

        Ok(Notifier {
            client,
            topic: String::from(topic),
            notifier_type,
        })
    }

    pub fn token<R>(
        pkcs8: &mut R,
        key_id: &str,
        team_id: &str,
        endpoint: Endpoint,
        topic: &str,
    ) -> Result<Notifier, Error>
    where
        R: Read,
    {
        let client = Client::token(pkcs8, key_id, team_id, endpoint)?;
        let notifier_type = NotifierType::Token;
        TOKEN_CONSUMERS.inc();

        Ok(Notifier {
            client,
            topic: String::from(topic),
            notifier_type,
        })
    }

    pub fn notify(&self, event: &PushNotification) -> Timeout<FutureResponse> {
        self.client.send_with_timeout(self.gen_payload(event), Duration::from_secs(3))
    }

    fn gen_payload<'a>(&'a self, event: &'a PushNotification) -> Payload<'a> {
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
            options.apns_id = Some(event.get_correlation_id());
        }
        if headers.has_apns_expiration() {
            options.apns_expiration = Some(headers.get_apns_expiration() as u64);
        }
        if headers.has_apns_topic() {
            options.apns_topic = Some(headers.get_apns_topic());
        } else {
            options.apns_topic = Some(&self.topic);
        }

        let mut payload = if notification_data.has_localized() {
            let alert_data = notification_data.get_localized();
            let mut builder =
                LocalizedNotificationBuilder::new(alert_data.get_title(), alert_data.get_body());

            if alert_data.has_title_loc_key() {
                builder.set_title_loc_key(alert_data.get_title_loc_key());
            }
            if !alert_data.get_title_loc_args().is_empty() {
                builder.set_title_loc_args(&alert_data.get_title_loc_args());
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
            if !alert_data.get_loc_args().is_empty() {
                builder.set_loc_args(&alert_data.get_loc_args());
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
