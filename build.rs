extern crate protoc_rust;

fn main() {
    protoc_rust::run(protoc_rust::Args {
        out_dir: "src/common/events",
        input: &[
            "third_party/events/schema/common/header.proto",
            "third_party/events/schema/common/map_field_entry.proto",
            "third_party/events/schema/notification/apple_config.proto",
            "third_party/events/schema/notification/push_notification.proto",
            "third_party/events/schema/notification/apple_notification.proto",
            "third_party/events/schema/notification/google_notification.proto",
            "third_party/events/schema/notification/notification_result.proto",
            "third_party/events/schema/notification/webpush_notification.proto",
        ],
        includes: &["third_party/events/schema/"],
        customize: protoc_rust::Customize {
            ..Default::default()
        },
    }).expect("protoc");
}

