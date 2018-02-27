use apns2::response::ErrorReason;
use events::apple_notification::ApnsResult_Reason;
use events::apple_notification::ApnsResult_Reason::*;
use events::apple_notification::ApnsResult_Status;
use events::apple_notification::ApnsResult_Status::*;

impl<'a> From<&'a ErrorReason> for ApnsResult_Reason {
    fn from(e: &'a ErrorReason) -> ApnsResult_Reason {
        match e {
            &ErrorReason::PayloadEmpty => PayloadEmpty,
            &ErrorReason::BadTopic => BadTopic,
            &ErrorReason::TopicDisallowed => TopicDisallowed,
            &ErrorReason::BadMessageId => BadMessageId,
            &ErrorReason::BadExpirationDate => BadExpirationDate,
            &ErrorReason::BadPriority => BadPriority,
            &ErrorReason::MissingDeviceToken => MissingDeviceToken,
            &ErrorReason::BadDeviceToken => BadDeviceToken,
            &ErrorReason::DeviceTokenNotForTopic => DeviceTokenNotForTopic,
            &ErrorReason::DuplicateHeaders => DuplicateHeaders,
            &ErrorReason::BadCertificateEnvironment => BadCertificateEnvironment,
            &ErrorReason::BadCertificate => BadCertificate,
            &ErrorReason::BadPath => BadPath,
            &ErrorReason::IdleTimeout => IdleTimeout,
            &ErrorReason::Shutdown => Shutdown,
            &ErrorReason::InternalServerError => InternalServerError,
            &ErrorReason::ServiceUnavailable => ServiceUnavailable,
            &ErrorReason::MissingTopic => MissingTopic,
            &ErrorReason::InvalidProviderToken => InvalidProviderToken,
            &ErrorReason::MissingProviderToken => MissingProviderToken,
            &ErrorReason::ExpiredProviderToken => ExpiredProviderToken,
            _ => Nothing,
        }
    }
}

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
