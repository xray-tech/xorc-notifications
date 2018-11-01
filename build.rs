use protoc_rust;

fn main() {
    protoc_rust::run(protoc_rust::Args {
        out_dir: "src/common/events",
        input: &[
            "third_party/events/common/rpc.proto",
            "third_party/events/common/rpc_decoder.proto",
            "third_party/events/application.proto",
            "third_party/events/http/http_request.proto",
            "third_party/events/http/http_response.proto",
            "third_party/events/notification/push_notification.proto",
            "third_party/events/notification/apple_notification.proto",
            "third_party/events/notification/google_notification.proto",
            "third_party/events/notification/webpush_notification.proto",
            "third_party/events/notification/push_result.proto",
        ],
        includes: &["third_party/events/"],
        customize: protoc_rust::Customize {
            ..Default::default()
        },
    }).expect("protoc");
}
