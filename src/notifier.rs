use fcm::*;
use events::push_notification::PushNotification;
use std::collections::HashMap;
use metrics::Metrics;
use std::sync::Arc;

pub struct Notifier<'a> {
    fcm_client: Client,
    metrics: Arc<Metrics<'a>>,
}

impl<'a> Notifier<'a> {
    pub fn new(metrics: Arc<Metrics<'a>>) -> Notifier<'a> {
        Notifier {
            fcm_client: Client::new(),
            metrics: metrics,
        }
    }

    pub fn send(&self, pn: &PushNotification) -> Result<FcmResponse, FcmError> {
        let notification = pn.get_google();

        let message = if notification.has_localized() {
            let localized   = notification.get_localized();
            let mut builder = NotificationBuilder::new(localized.get_title());

            if localized.has_tag()           { builder.tag(localized.get_tag());                     }
            if localized.has_body()          { builder.body(localized.get_body());                   }
            if localized.has_icon()          { builder.icon(localized.get_icon());                   }
            if localized.has_sound()         { builder.sound(localized.get_sound());                 }
            if localized.has_badge()         { builder.badge(localized.get_badge());                 }
            if localized.has_color()         { builder.color(localized.get_color());                 }
            if localized.has_click_action()  { builder.click_action(localized.get_click_action());   }
            if localized.has_body_loc_key()  { builder.body_loc_key(localized.get_body_loc_key());   }
            if localized.has_title_loc_key() { builder.title_loc_key(localized.get_title_loc_key()); }

            if localized.get_title_loc_args().len() > 0 {
                builder.title_loc_args(localized.get_title_loc_args().to_vec());
            }

            if localized.get_body_loc_args().len() > 0 {
                builder.body_loc_args(localized.get_body_loc_args().to_vec());
            }

            Message::new(pn.get_device_token()).notification(builder.finalize())
        } else {
            let key_values = notification.get_message().get_key_value().iter();

            let data = key_values.fold(HashMap::new(), |mut acc, kv| {
                acc.insert(kv.get_key(), kv.get_value());
                acc
            });

            Message::new(pn.get_device_token()).data(data)
        };

        self.metrics.timers.response_time.time(|| {
            self.fcm_client.send(message, "AIzaSyDEXS1bYcNN9a2C-PeamjlmRmZ89CTYUW4")
        })
    }
}
