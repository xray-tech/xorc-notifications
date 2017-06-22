use std::process::Command;

fn main() {
    Command::new("protoc").args(&["--rust_out", "src/events", "--proto_path",
                                "./third_party/events/schema/",
                                "./third_party/events/schema/common/header.proto",
                                "./third_party/events/schema/common/map_field_entry.proto",
                                "./third_party/events/schema/notification/push_notification.proto",
                                "./third_party/events/schema/notification/google_notification.proto",
                                "./third_party/events/schema/notification/apple_notification.proto",
    ]).status().unwrap();
}
