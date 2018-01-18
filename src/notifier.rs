use fcm::*;
use events::push_notification::PushNotification;
use events::google_notification::GoogleNotification_Priority;
use std::collections::HashMap;
use tokio_core::reactor::Core;
use futures::sync::mpsc::{Sender, Receiver};
use futures::{Future, Stream, Sink};
use producer::FcmData;
use metrics::{RESPONSE_TIMES_HISTOGRAM, REQUEST_COUNTER};

pub struct Notifier {}

impl Notifier {
    pub fn new() -> Notifier {
        Notifier {}
    }

    pub fn run(&self, consumer_rx: Receiver<(Option<String>, PushNotification)>, producer_tx: Sender<FcmData>) {
        let mut core = Core::new().unwrap();
        let handle = core.handle();
        let client = Client::new(&handle).unwrap();

        let sender = consumer_rx.for_each(|(api_key, event)| {
            let tx = producer_tx.clone();

            match api_key {
                Some(ref key) => {
                    REQUEST_COUNTER.with_label_values(&["requested",
                                                        event.get_application_id(),
                                                        event.get_campaign_id()]).inc();

                    let message = Self::build_message(&event, key);
                    let timer = RESPONSE_TIMES_HISTOGRAM.start_timer();

                    let work = client.send(message).then(|res| {
                        timer.observe_duration();
                        tx.send((event, Some(res)))
                    }).then(|_| Ok(()));

                    handle.spawn(work);
                },
                None => {
                    let work = tx.send((event, None)).then(|_| Ok(()));

                    handle.spawn(work);
                }
            }

            Ok(())
        });

        core.run(sender).unwrap();
    }

    fn build_message(pn: &PushNotification, api_key: &str) -> Message {
        let notification = pn.get_google();
        let mut message  = MessageBuilder::new(api_key, pn.get_device_token());

        if notification.has_localized() {
            let localized   = notification.get_localized();
            let mut builder = NotificationBuilder::new();

            if localized.has_title()         { builder.title(localized.get_title());                 }
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

        if notification.get_registration_ids().len() > 0 {
            message.registration_ids(notification.get_registration_ids().to_vec());
        }

        if notification.has_collapse_key() {
            message.collapse_key(notification.get_collapse_key());
        }

        match notification.get_priority() {
            GoogleNotification_Priority::Normal => {
                message.priority(Priority::Normal);
            },
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
