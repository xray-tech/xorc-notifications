use std::process::Command;

fn main() {
    Command::new("protoc").args(&["--rust_out", "src/events", "--proto_path",
                                "../../../third_party/events/schema/",
                                "../../../third_party/events/schema/notification/push_notification.proto",
                                "../../../third_party/events/schema/notification/apns_result.proto",
                                "../../../third_party/events/schema/notification/apns_headers.proto",
    ]).status().unwrap();
}

