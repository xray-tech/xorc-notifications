use a2::response::ErrorReason as ApnsReason;

pub mod push_notification;
pub mod apple_notification;
pub mod apple_config;
pub mod google_notification;
pub mod webpush_notification;
pub mod notification_result;
pub mod map_field_entry;
pub mod header;

use self::apple_notification::{
    ApnsResult_Reason,
    ApnsResult_Reason::*,
    ApnsResult_Status,
    ApnsResult_Status::*,
};

impl From<u16> for ApnsResult_Status {
    fn from(status: u16) -> ApnsResult_Status {
        match status {
            200 => Success,
            400 => BadRequest,
            403 => Forbidden,
            405 => MethodNotAllowed,
            410 => Unregistered,
            413 => PayloadTooLarge,
            429 => TooManyRequests,
            _ => Error,
        }
    }
}

impl<'a> From<&'a ApnsReason> for ApnsResult_Reason {
    fn from(e: &'a ApnsReason) -> ApnsResult_Reason {
        match e {
            &ApnsReason::PayloadEmpty => PayloadEmpty,
            &ApnsReason::BadTopic => BadTopic,
            &ApnsReason::TopicDisallowed => TopicDisallowed,
            &ApnsReason::BadMessageId => BadMessageId,
            &ApnsReason::BadExpirationDate => BadExpirationDate,
            &ApnsReason::BadPriority => BadPriority,
            &ApnsReason::MissingDeviceToken => MissingDeviceToken,
            &ApnsReason::BadDeviceToken => BadDeviceToken,
            &ApnsReason::DeviceTokenNotForTopic => DeviceTokenNotForTopic,
            &ApnsReason::DuplicateHeaders => DuplicateHeaders,
            &ApnsReason::BadCertificateEnvironment => BadCertificateEnvironment,
            &ApnsReason::BadCertificate => BadCertificate,
            &ApnsReason::BadPath => BadPath,
            &ApnsReason::IdleTimeout => IdleTimeout,
            &ApnsReason::Shutdown => Shutdown,
            &ApnsReason::InternalServerError => InternalServerError,
            &ApnsReason::ServiceUnavailable => ServiceUnavailable,
            &ApnsReason::MissingTopic => MissingTopic,
            &ApnsReason::InvalidProviderToken => InvalidProviderToken,
            &ApnsReason::MissingProviderToken => MissingProviderToken,
            &ApnsReason::ExpiredProviderToken => ExpiredProviderToken,
            _ => Nothing,
        }
    }
}

