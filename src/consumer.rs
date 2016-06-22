use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::sync::mpsc::Sender;
use std::thread;
use amqp::{Session, Channel, Table, Basic, Options};
use events::push_notification::PushNotification;
use protobuf::parse_from_bytes;
use config::Config;
use hyper::error::Error;
use notifier::Notifier;
use producer::FcmData;

pub struct Consumer<'a> {
    channel: Channel,
    session: Session,
    notifier: Notifier<'a>,
    control: Arc<AtomicBool>,
    config: Arc<Config>,
    tx: Sender<FcmData>,
}

impl<'a> Drop for Consumer<'a> {
    fn drop(&mut self) {
        let _ = self.channel.close(200, "Bye!");
        let _ = self.session.close(200, "Good bye!");
    }
}

impl<'a> Consumer<'a> {
    pub fn new(control: Arc<AtomicBool>, config: Arc<Config>, notifier: Notifier<'a>,
               tx: Sender<FcmData>) -> Consumer {
        let mut session = Session::new(Options {
            vhost: &config.rabbitmq.vhost,
            host: &config.rabbitmq.host,
            port: config.rabbitmq.port,
            login: &config.rabbitmq.login,
            password: &config.rabbitmq.password, .. Default::default()
        }).unwrap();

        let mut channel = session.open_channel(1).unwrap();

        channel.queue_declare(
            &*config.rabbitmq.queue,
            false, // passive
            true,  // durable
            false, // exclusive
            false, // auto_delete
            false, // nowait
            Table::new()).unwrap();

        channel.exchange_declare(
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.exchange_type,
            false, // passive
            true,  // durable
            false, // auto_delete
            false, // internal
            false, // nowait
            Table::new()).unwrap();

        channel.queue_bind(
            &*config.rabbitmq.queue,
            &*config.rabbitmq.exchange,
            &*config.rabbitmq.routing_key,
            false, // nowait
            Table::new()).unwrap();

        Consumer {
            channel: channel,
            session: session,
            notifier: notifier,
            control: control,
            config: config,
            tx: tx,
        }
    }

    pub fn consume(&mut self) -> Result<(), Error> {
        while self.control.load(Ordering::Relaxed) {
            for result in self.channel.basic_get(&self.config.rabbitmq.queue, false) {
                if let Ok(event) = parse_from_bytes::<PushNotification>(&result.body) {
                    let response = self.notifier.send(&event);

                    self.tx.send((event, response)).unwrap();
                } else {
                    error!("Broken protobuf data");
                }

                result.ack();

                if !self.control.load(Ordering::Relaxed) { break; }
            }

            thread::park_timeout(Duration::from_millis(100));
        }

        Ok(())
    }
}

