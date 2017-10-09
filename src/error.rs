use web_push::WebPushError;
use events::webpush_notification::WebPushResult_Error;
use events::notification_result::NotificationResult_Error;

impl<'a> From<&'a WebPushError> for WebPushResult_Error {
    fn from(e: &'a WebPushError) -> WebPushResult_Error {
        match *e {
            WebPushError::Unspecified      => WebPushResult_Error::Unspecified,
            WebPushError::Unauthorized     => WebPushResult_Error::Unauthorized,
            WebPushError::BadRequest       => WebPushResult_Error::BadRequest,
            WebPushError::ServerError(_)   => WebPushResult_Error::ServerError,
            WebPushError::NotImplemented   => WebPushResult_Error::NotImplemented,
            WebPushError::InvalidUri       => WebPushResult_Error::InvalidUri,
            WebPushError::TimeoutError     => WebPushResult_Error::TimeoutError,
            WebPushError::EndpointNotValid => WebPushResult_Error::EndpointNotValid,
            WebPushError::EndpointNotFound => WebPushResult_Error::EndpointNotFound,
            WebPushError::PayloadTooLarge  => WebPushResult_Error::PayloadTooLarge,
            WebPushError::TlsError         => WebPushResult_Error::TlsError,
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
