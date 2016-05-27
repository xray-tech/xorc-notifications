extern crate apns2;

mod notifier;

use notifier::Apns2Notifier;

fn main() {
    let notifier = Apns2Notifier::new("./cert/push_cert.pem", "./cert/push_key.pem");
    notifier.send("xxx...xxx", "Kulli");
}
