use apns2::client::{APNSError, APNSStatus};
use events::apple_notification::ApnsResult_Reason;
use events::apple_notification::ApnsResult_Reason::*;
use events::apple_notification::ApnsResult_Status;
use events::apple_notification::ApnsResult_Status::*;

impl<'a> From<&'a APNSError> for ApnsResult_Reason {
    fn from(e: &'a APNSError) -> ApnsResult_Reason {
        match *e {
            APNSError::PayloadEmpty              => PayloadEmpty,
            APNSError::BadTopic                  => BadTopic,
            APNSError::TopicDisallowed           => TopicDisallowed,
            APNSError::BadMessageId              => BadMessageId,
            APNSError::BadExpirationDate         => BadExpirationDate,
            APNSError::BadPriority               => BadPriority,
            APNSError::MissingDeviceToken        => MissingDeviceToken,
            APNSError::BadDeviceToken            => BadDeviceToken,
            APNSError::DeviceTokenNotForTopic    => DeviceTokenNotForTopic,
            APNSError::DuplicateHeaders          => DuplicateHeaders,
            APNSError::BadCertificateEnvironment => BadCertificateEnvironment,
            APNSError::BadCertificate            => BadCertificate,
            APNSError::BadPath                   => BadPath,
            APNSError::IdleTimeout               => IdleTimeout,
            APNSError::Shutdown                  => Shutdown,
            APNSError::InternalServerError       => InternalServerError,
            APNSError::ServiceUnavailable        => ServiceUnavailable,
            APNSError::MissingTopic              => MissingTopic,
            APNSError::InvalidProviderToken      => InvalidProviderToken,
            APNSError::MissingProviderToken      => MissingProviderToken,
            APNSError::ExpiredProviderToken      => ExpiredProviderToken,
            APNSError::PayloadTooLarge           => Nothing,
            APNSError::Unregistered              => Nothing,
            APNSError::Forbidden                 => Nothing,
            APNSError::MethodNotAllowed          => Nothing,
            APNSError::TooManyRequests           => Nothing,
            APNSError::SslError(_)               => Nothing,
            APNSError::ClientConnectError(_)     => Nothing,
        }
    }
}

impl<'a> From<&'a APNSStatus> for ApnsResult_Status {
    fn from(status: &'a APNSStatus) -> ApnsResult_Status {
        match *status {
            APNSStatus::Success          => Success,
            APNSStatus::BadRequest       => BadRequest,
            APNSStatus::MissingChannel   => MissingChannel,
            APNSStatus::Timeout          => Timeout,
            APNSStatus::Unknown          => Unknown,
            APNSStatus::Unregistered     => Unregistered,
            APNSStatus::Forbidden        => Forbidden,
            APNSStatus::MethodNotAllowed => MethodNotAllowed,
            APNSStatus::PayloadTooLarge  => PayloadTooLarge,
            APNSStatus::TooManyRequests  => TooManyRequests,
            _                            => Error,
        }
    }
}
