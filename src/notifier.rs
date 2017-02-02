use apns2::client::{TokenClient, CertificateClient, ProviderResponse};
use apns2::notification::{NotificationOptions, Notification};
use apns2::payload::{APSAlert, Payload, APSLocalizedAlert, CustomData};
use apns2::device_token::DeviceToken;
use events::push_notification::PushNotification;
use std::io::Read;
use rustc_serialize::json::{Json};

pub struct CertificateNotifier {
    pub client: CertificateClient,
}

pub struct TokenNotifier {
    pub client: TokenClient,
}

impl CertificateNotifier {
    pub fn new<R: Read>(mut certificate: R, mut private_key: R, sandbox: bool) -> CertificateNotifier {
        CertificateNotifier {
            client: CertificateClient::new(sandbox, &mut certificate, &mut private_key).unwrap(),
        }
    }

    pub fn send(&self, event: &PushNotification, apns_topic: &str) -> ProviderResponse {
        let payload      = gen_payload(event);
        let options      = gen_options(event, apns_topic);
        let device_token = DeviceToken::new(event.get_device_token());

        self.client.push(Notification::new(payload, device_token, options))
    }
}

impl TokenNotifier {
    pub fn new(sandbox: bool) -> TokenNotifier {
        TokenNotifier {
            client: TokenClient::new(sandbox, "/etc/ssl/cert.pem").unwrap(),
        }
    }

    pub fn send(&self, event: &PushNotification, apns_topic: &str, apns_token: &str) -> ProviderResponse {
        let payload      = gen_payload(event);
        let options      = gen_options(event, apns_topic);
        let device_token = DeviceToken::new(event.get_device_token());

        self.client.push(Notification::new(payload, device_token, options), apns_token)
    }
}

fn gen_options<'a>(event: &'a PushNotification, apns_topic: &'a str) -> NotificationOptions<'a> {
    let notification_data = event.get_apple();
    let headers = notification_data.get_headers();

    NotificationOptions {
        apns_priority:   if headers.has_apns_priority()   { Some(headers.get_apns_priority())   } else { None },
        apns_id:         if event.has_correlation_id()    { Some(event.get_correlation_id())    } else { None },
        apns_expiration: if headers.has_apns_expiration() { Some(headers.get_apns_expiration()) } else { None },
        apns_topic:      if headers.has_apns_topic()      { Some(headers.get_apns_topic())      } else { Some(apns_topic) },
        ..Default::default()
    }
}

fn gen_payload(event: &PushNotification) -> Payload {
    let notification_data = event.get_apple();

    let badge = if notification_data.has_badge() {
        notification_data.get_badge()
    } else { 1u32 };

    let sound = if notification_data.has_sound() {
        notification_data.get_sound()
    } else { "default" };

    let category = if notification_data.has_category() {
        Some(notification_data.get_category().to_string())
    } else { None };

    let custom_data = if notification_data.has_custom_data() {
        let custom_data = notification_data.get_custom_data();

        match Json::from_str(custom_data.get_body()) {
            Ok(json) => Some(CustomData {
                key: custom_data.get_key().to_string(),
                body: json,
            }),
            Err(e) => {
                error!("Non-json custom data: {:?}", e);
                None
            },
        }
    } else { None };

    if notification_data.has_localized() {
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

        Payload::new(alert, badge, sound, category, custom_data)
    } else if notification_data.has_silent() {
        Payload::new_silent_notification(custom_data)
    } else {
        let alert = APSAlert::Plain(notification_data.get_plain().to_string());

        Payload::new(alert, badge, sound, category, custom_data)
    }
}
