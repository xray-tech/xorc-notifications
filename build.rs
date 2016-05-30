use std::process::Command;

fn main() {
    Command::new("protoc").args(&["--rust_out", "src/events", "--proto_path",
                                "../../../third_party/events/schema/",
                                "../../../third_party/events/schema/notification/apn.proto"
    ]).status().unwrap();
}

