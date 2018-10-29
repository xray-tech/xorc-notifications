use self::{
    http_request::HttpRequest_HttpVerb::{self, *},
    push_result::{
        PushResult,
        PushResult_ResponseAction as ResponseAction
    },
    push_notification::PushNotification,
};

pub mod apple_notification;
pub mod application;
pub mod google_notification;
pub mod rpc;
pub mod rpc_decoder;
pub mod push_notification;
pub mod push_result;
pub mod webpush_notification;
pub mod http_request;
pub mod http_response;

impl Into<PushResult> for (PushNotification, ResponseAction) {
    fn into(self) -> PushResult {
        let (mut event, response_action) = self;

        let mut header = rpc::Response::new();
        header.set_field_type("notification.PushResult".to_string());
        header.set_request(event.take_header());

        let mut result = PushResult::new();
        result.set_header(header);
        result.set_response_action(response_action);

        result
    }
}

impl AsRef<str> for HttpRequest_HttpVerb {
    fn as_ref(&self) -> &str {
        match self {
            GET => "GET",
            POST => "POST",
            PUT => "PUT",
            DELETE => "DELETE",
            PATCH => "PATCH",
            OPTIONS => "OPTIONS",
        }
    }
}
