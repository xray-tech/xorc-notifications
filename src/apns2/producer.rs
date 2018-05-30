use rdkafka::{
    config::ClientConfig,
    producer::{FutureProducer, DeliveryFuture},
};

use gelf::{
    Level as GelfLevel,
    Message as GelfMessage,
    Error as GelfError
};

use a2::{
    response::Response,
    error::Error,
};

use common::{
    logger::LogAction,
    metrics::*,
    events::{
        header::Header,
        apple_notification::*,
        notification_result::{NotificationResult, NotificationResult_Error},
        apple_notification::ApnsResult_Reason::*,
        apple_notification::ApnsResult_Status::*,
        push_notification::PushNotification,
    },
};

use protobuf::{Message as ProtoMessage};
use heck::SnakeCase;
use chrono::offset::Utc;

use ::{GLOG, CONFIG};

pub struct ApnsProducer {
    producer: FutureProducer,
}

impl ApnsProducer {
    pub fn new() -> ApnsProducer {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &CONFIG.kafka.brokers)
            .set("produce.offset.report", "true")
            .create()
            .expect("Producer creation error");

        ApnsProducer { producer }
    }

    fn get_retry_after(event: &PushNotification) -> u32 {
        if event.has_retry_count() {
            let base: u32 = 2;
            base.pow(event.get_retry_count())
        } else {
            1
        }
    }

    pub fn handle_ok(&self, mut event: PushNotification, response: Response) -> DeliveryFuture {
        CALLBACKS_COUNTER.with_label_values(&["success"]).inc();

        let _ = self.log_result(
            "Successfully sent a push notification",
            &event,
            Some(&response),
            None
        );

        let mut apns_result = ApnsResult::new();

        apns_result.set_successful(true);
        apns_result.set_status(ApnsResult_Status::Success);

        event.mut_apple().set_result(apns_result);

        self.publish(event, &CONFIG.kafka.output_topic)
    }

    pub fn handle_err(&self, mut event: PushNotification, response: Response) -> DeliveryFuture {
        let _ = self.log_result(
            "Error sending a push notification",
            &event,
            Some(&response),
            None
        );

        let mut apns_result = ApnsResult::new();
        let status: ApnsResult_Status = response.code.into();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        if let Some(ref error) = response.error {
            apns_result.set_reason((&error.reason).into());

            if let Some(ts) = error.timestamp {
                apns_result.set_timestamp(ts as i64);
            }
        };

        match response.error {
            Some(ref reason) => {
                let reason_label = format!("{:?}", reason.reason).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&reason_label]).inc();
            }
            None => {
                let status_label = format!("{:?}", status).to_snake_case();
                CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();
            }
        }

        let topic = match apns_result.get_reason() {
            InternalServerError | Shutdown | ServiceUnavailable | ExpiredProviderToken => {
                let ra = Self::get_retry_after(&event);
                event.set_retry_after(ra);

                &CONFIG.kafka.retry_topic
            }
            _ => match apns_result.get_status() {
                Timeout | Unknown | Forbidden => {
                    let ra = Self::get_retry_after(&event);
                    event.set_retry_after(ra);

                    &CONFIG.kafka.retry_topic
                }
                _ => &CONFIG.kafka.output_topic,
            },
        };

        event.mut_apple().set_result(apns_result);
        self.publish(event, topic)
    }

    pub fn handle_fatal(&self, mut event: PushNotification, error: Error) -> DeliveryFuture {
        let mut apns_result = ApnsResult::new();

        let status = match error {
            Error::TimeoutError => ApnsResult_Status::Timeout,
            Error::ConnectionError => ApnsResult_Status::MissingChannel,
            _ => ApnsResult_Status::Unknown,
        };

        let status_label = format!("{:?}", status).to_snake_case();

        apns_result.set_status(status);
        apns_result.set_successful(false);

        CALLBACKS_COUNTER.with_label_values(&[&status_label]).inc();

        event.mut_apple().set_result(apns_result);

        let ra = Self::get_retry_after(&event);
        event.set_retry_after(ra);

        self.publish(event, &CONFIG.kafka.retry_topic)
    }

    fn publish(&self, event: PushNotification, topic: &str) -> DeliveryFuture {
        let response = event.get_apple().get_result();
        let mut header = Header::new();

        header.set_created_at(Utc::now().timestamp_millis());
        header.set_source(String::from("apns"));
        header.set_recipient_id(String::from("MISSING TODO TODO TODO"));
        header.set_field_type(String::from("notification.NotificationResult"));

        let mut result_event = NotificationResult::new();
        result_event.set_header(header);
        result_event.set_universe(String::from(event.get_universe()));
        result_event.set_correlation_id(String::from(event.get_correlation_id()));

        match response.get_status() {
            Success => {
                result_event.set_delete_user(false);
                result_event.set_successful(true);
            }
            Unregistered => {
                result_event.set_delete_user(true);
                result_event.set_successful(false);
                result_event.set_error(NotificationResult_Error::Unsubscribed);
            }
            _ => {
                match response.get_reason() {
                    DeviceTokenNotForTopic | BadDeviceToken => {
                        result_event.set_delete_user(true);
                        result_event.set_successful(false);
                        result_event.set_reason(format!("{:?}", response.get_reason()));
                        result_event.set_error(NotificationResult_Error::Unsubscribed);
                    },
                    _ => {
                        result_event.set_delete_user(false);
                        result_event.set_successful(false);
                        result_event.set_reason(format!("{:?}", response.get_status()));
                        result_event.set_error(NotificationResult_Error::Other);
                    }
                }
            }
        }

        self.producer.send_copy::<Vec<u8>, ()>(
            topic,
            None,
            Some(&result_event.write_to_bytes().unwrap()),
            None,
            None,
            1000
        )
    }

    fn log_result(
        &self,
        title: &str,
        event: &PushNotification,
        response: Option<&Response>,
        error: Option<Error>
    ) -> Result<(), GelfError>
    {
        let mut test_msg = GelfMessage::new(String::from(title));
        test_msg.set_metadata("action", format!("{:?}", LogAction::NotificationResult))?;

        test_msg
            .set_full_message(format!("{:?}", event))
            .set_level(GelfLevel::Informational)
            .set_metadata("correlation_id", format!("{}", event.get_correlation_id()))?
            .set_metadata("device_token", format!("{}", event.get_device_token()))?
            .set_metadata("app_id", format!("{}", event.get_application_id()))?
            .set_metadata("campaign_id", format!("{}", event.get_campaign_id()))?
            .set_metadata(
                "event_source",
                String::from(event.get_header().get_source()),
            )?;

        if let Some(r) = response {
            match r.code {
                200 => {
                    test_msg.set_metadata("successful", String::from("true"))?;
                }
                code => {
                    let error: ApnsResult_Status = code.into();
                    test_msg.set_metadata("successful", String::from("false"))?;
                    test_msg.set_metadata("error", format!("{:?}", error))?;

                    if let Some(ref reason) = r.error {
                        test_msg.set_metadata("reason", format!("{:?}", reason.reason))?;
                    }
                }
            }
        } else {
            test_msg.set_metadata("successful", String::from("false"))?;

            if let Some(e) = error {
                test_msg.set_metadata("error", format!("{:?}", e))?;
            }
        }

        GLOG.log_message(test_msg);

        Ok(())
    }
}

impl Clone for ApnsProducer {
    fn clone(&self) -> Self {
        ApnsProducer {
            producer: self.producer.clone(),
        }
    }
}
