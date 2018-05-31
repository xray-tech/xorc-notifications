use a2::response::ErrorReason as ApnsReason;
use web_push::WebPushError;

use self::{
    webpush_notification::WebPushResult_Error,
    notification_result::NotificationResult_Error,
    apple_notification::{
        ApnsResult_Reason,
        ApnsResult_Reason::*,
        ApnsResult_Status,
        ApnsResult_Status::*,
    },
};

pub mod push_notification;
pub mod apple_notification;
pub mod apple_config;
pub mod google_config;
pub mod google_notification;
pub mod webpush_notification;
pub mod web_push_config;
pub mod notification_result;
pub mod map_field_entry;
pub mod header;


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

impl<'a> From<&'a WebPushError> for WebPushResult_Error {
    fn from(e: &'a WebPushError) -> WebPushResult_Error {
        match *e {
            WebPushError::Unspecified        => WebPushResult_Error::Unspecified,
            WebPushError::Unauthorized       => WebPushResult_Error::Unauthorized,
            WebPushError::BadRequest(_)      => WebPushResult_Error::BadRequest,
            WebPushError::ServerError(_)     => WebPushResult_Error::ServerError,
            WebPushError::NotImplemented     => WebPushResult_Error::NotImplemented,
            WebPushError::InvalidUri         => WebPushResult_Error::InvalidUri,
            WebPushError::TimeoutError       => WebPushResult_Error::TimeoutError,
            WebPushError::EndpointNotValid   => WebPushResult_Error::EndpointNotValid,
            WebPushError::EndpointNotFound   => WebPushResult_Error::EndpointNotFound,
            WebPushError::PayloadTooLarge    => WebPushResult_Error::PayloadTooLarge,
            WebPushError::TlsError           => WebPushResult_Error::TlsError,
            WebPushError::InvalidPackageName => WebPushResult_Error::InvalidPackageName,
            WebPushError::InvalidTtl         => WebPushResult_Error::InvalidTtl,
            WebPushError::MissingCryptoKeys  => WebPushResult_Error::MissingCryptoKeys,
            WebPushError::InvalidCryptoKeys  => WebPushResult_Error::InvalidCryptoKeys,
            WebPushError::InvalidResponse    => WebPushResult_Error::InvalidResponse,
            WebPushError::SslError           => WebPushResult_Error::Other,
            WebPushError::IoError            => WebPushResult_Error::Other,
            WebPushError::Other(_)           => WebPushResult_Error::Other,
        }
    }
}

impl<'a> From<&'a WebPushResult_Error> for NotificationResult_Error {
    fn from(e: &'a WebPushResult_Error) -> NotificationResult_Error {
        match *e {
            WebPushResult_Error::EndpointNotFound => NotificationResult_Error::Unsubscribed,
            WebPushResult_Error::EndpointNotValid => NotificationResult_Error::Unsubscribed,
            _ => NotificationResult_Error::Other,
        }
    }
}
