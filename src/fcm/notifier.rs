use common::events::{google_notification::GoogleNotification_Priority,
                     push_notification::PushNotification};

use fcm::*;
use std::collections::HashMap;

pub struct Notifier {
    client: Client,
}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier {
            client: Client::new().unwrap(),
        }
    }

    pub fn notify(&self, event: &PushNotification, api_key: &str) -> FutureResponse {
        self.client.send(Self::gen_payload(event, api_key))
    }

    fn gen_payload<'a>(pn: &'a PushNotification, api_key: &'a str) -> Message<'a> {
        let notification = pn.get_google();
        let mut message = MessageBuilder::new(api_key, pn.get_device_token());

        if notification.has_localized() {
            let localized = notification.get_localized();
            let mut builder = NotificationBuilder::new();

            if localized.has_title() {
                builder.title(localized.get_title());
            }
            if localized.has_tag() {
                builder.tag(localized.get_tag());
            }
            if localized.has_body() {
                builder.body(localized.get_body());
            }
            if localized.has_icon() {
                builder.icon(localized.get_icon());
            }
            if localized.has_sound() {
                builder.sound(localized.get_sound());
            }
            if localized.has_badge() {
                builder.badge(localized.get_badge());
            }
            if localized.has_color() {
                builder.color(localized.get_color());
            }
            if localized.has_click_action() {
                builder.click_action(localized.get_click_action());
            }
            if localized.has_body_loc_key() {
                builder.body_loc_key(localized.get_body_loc_key());
            }
            if localized.has_title_loc_key() {
                builder.title_loc_key(localized.get_title_loc_key());
            }

            if !localized.get_title_loc_args().is_empty() {
                builder.title_loc_args(localized.get_title_loc_args());
            }

            if !localized.get_body_loc_args().is_empty() {
                builder.body_loc_args(localized.get_body_loc_args());
            }

            let key_values = localized.get_data().iter();
            let data = key_values.fold(HashMap::new(), |mut acc, kv| {
                acc.insert(kv.get_key(), kv.get_value());
                acc
            });
            if !data.is_empty() {
                if let Err(e) = message.data(&data) {
                    error!("Couldn't encode custom data to the message: {:?}", e);
                }
            }
            message.notification(builder.finalize());
        } else {
            let key_values = notification.get_message().get_data().iter();

            let data = key_values.fold(HashMap::new(), |mut acc, kv| {
                acc.insert(kv.get_key(), kv.get_value());
                acc
            });

            if let Err(e) = message.data(&data) {
                error!("Couldn't encode custom data to the message: {:?}", e);
            }
        }

        if !notification.get_registration_ids().is_empty() {
            message.registration_ids(notification.get_registration_ids());
        }

        if notification.has_collapse_key() {
            message.collapse_key(notification.get_collapse_key());
        }

        match notification.get_priority() {
            GoogleNotification_Priority::Normal => {
                message.priority(Priority::Normal);
            }
            GoogleNotification_Priority::High => {
                message.priority(Priority::High);
            }
        }

        if notification.has_content_available() {
            message.content_available(notification.get_content_available());
        }

        if notification.has_delay_while_idle() {
            message.delay_while_idle(notification.get_delay_while_idle());
        }

        if notification.has_time_to_live() {
            message.time_to_live(notification.get_time_to_live());
        }

        if notification.has_restricted_package_name() {
            message.restricted_package_name(notification.get_restricted_package_name());
        }

        if notification.has_dry_run() {
            message.dry_run(notification.get_dry_run());
        }

        message.finalize()
    }
}
