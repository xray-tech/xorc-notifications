use apns2::*;
use events::push_notification::PushNotification;
use std::io::Read;

pub struct Apns2Notifier {
    apns2_provider: Provider,
}

impl Apns2Notifier {
    pub fn new<R: Read>(mut certificate: R, mut private_key: R, sandbox: &bool) -> Apns2Notifier {
        Apns2Notifier {
            apns2_provider: Provider::from_reader(*sandbox, &mut certificate, &mut private_key),
        }
    }

    pub fn send(&self, event: &PushNotification) -> AsyncResponse {
        let token             = DeviceToken::new(event.get_device_token());
        let notification_data = event.get_apple();
        let headers           = notification_data.get_headers();

        let options = NotificationOptions {
            apns_priority:   if headers.has_apns_priority()   { Some(headers.get_apns_priority())   } else { None },
            apns_id:         if event.has_correlation_id()    { Some(event.get_correlation_id())    } else { None },
            apns_expiration: if headers.has_apns_expiration() { Some(headers.get_apns_expiration()) } else { None },
            apns_topic:      if headers.has_apns_topic()      { Some(headers.get_apns_topic())      } else { None }, ..Default::default()
        };

        let badge = if notification_data.has_badge() {
            notification_data.get_badge()
        } else { 1u32 };

        let sound = if notification_data.has_sound() {
            notification_data.get_sound()
        } else { "default" };

        let category = if notification_data.has_category() {
            Some(notification_data.get_category().to_string())
        } else { None };

        let payload: Payload = if notification_data.has_localized() {
            let alert_data = notification_data.get_localized();

            let title_loc_key = if alert_data.has_title_loc_key() {
                Some(alert_data.get_title_loc_key().to_string())
            } else { None };

            let title_loc_args = if alert_data.get_title_loc_args().len() > 0 {
                Some(alert_data.get_title_loc_args().iter().map(|a| a.to_string()).collect())
            } else { None };

            let action_loc_key = if alert_data.has_action_loc_key() {
                Some(alert_data.get_action_loc_key().to_string())
            } else { None };

            let launch_image = if alert_data.has_launch_image() {
                Some(alert_data.get_launch_image().to_string())
            } else { None };

            let loc_key = if alert_data.has_loc_key() {
                Some(alert_data.get_loc_key().to_string())
            } else { None };

            let loc_args = if alert_data.get_loc_args().len() > 0 {
                Some(alert_data.get_loc_args().iter().map(|a| a.to_string()).collect())
            } else { None };

            let alert = APSAlert::Localized(
                APSLocalizedAlert {
                    title: alert_data.get_title().to_string(),
                    body: alert_data.get_body().to_string(),
                    title_loc_key: title_loc_key,
                    title_loc_args: title_loc_args,
                    action_loc_key: action_loc_key,
                    loc_key: loc_key,
                    loc_args: loc_args,
                    launch_image: launch_image,
                });

            Payload::new(alert, badge, sound, category)
        } else if notification_data.has_silent() {
            Payload::new_silent_notification()
        } else {
            let alert = APSAlert::Plain(notification_data.get_plain().to_string());

            Payload::new(alert, badge, sound, category)
        };

        self.apns2_provider.push(Notification::new(payload, token, options))
    }
}
