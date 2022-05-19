mod request_consumer;
mod response_producer;

pub use self::request_consumer::{EventHandler, RequestConsumer};
pub use self::response_producer::ResponseProducer;
pub use rdkafka::producer::DeliveryFuture;

use futures::Future;
//use futures_lite::future::FutureExt;
use core::pin::Pin;
use core::task::{Poll, Context};
use std::sync::{Arc,Mutex};
//use rdkafka::admin::ConfigSource::Default;


#[derive(Deserialize, Debug)]
pub struct Config {
    /// Kafka topic for incoming `PushNotification` events, triggering a push
    /// notification to be sent.
    pub input_topic: String,
    /// Kafka topic for incoming `Application` events, holding the tenant
    /// configuration.
    pub config_topic: String,
    /// Kafka topic for push notification responses.
    pub output_topic: String,
    /// Kafka consumer group ID.
    pub group_id: String,
    /// A comma-separated list of Kafka brokers to connect.
    pub brokers: String,
}

pub enum Erro {
    Ok,
    Err
}

pub async fn to_async<T>(data: T) -> T {
    data
}

impl Unpin for Erro {}

pub struct Rdy {
    pub inner: Erro
}

impl Unpin for Rdy {}

impl Rdy {
    pub fn get(&self) -> Erro{
        match &self.inner {
            Erro::Ok => Erro::Ok,
            Erro::Err=> Erro::Err
        }
    }

    pub fn new(item: &Erro) -> Rdy {
        Rdy{
            inner: match item {
                Erro::Ok=> Erro::Ok,
                Erro::Err => Erro::Err
            }
        }
    }
}

impl Future for Rdy {
    type Output = Erro;

    #[inline]
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Erro> {
        Poll::Ready(self.get())
    }
}

pub struct Whoosh <T> {
    inner: Arc<Mutex<Option<Option<Option<Arc<T>>>>>>, // really I should just make this an enum, shouldn't I
}

impl <T> Unpin for Whoosh <T> {}

impl <T> Whoosh <T>{
    pub fn get(&self) -> Arc<Mutex<Option<Option<Option<Arc<T>>>>>> {
        return self.inner.clone();
    }

    pub fn new(item: Arc<Mutex<Option<Option<Option<Arc<T>>>>>>) -> Self {
        return Whoosh{
            inner: item
        }
    }
}

impl <T> Future for Whoosh <T>{
    type Output = Result<(),Result<Arc<T>,()>>;

    fn poll(self: Pin<&mut Self>,_cx: &mut Context<'_>) -> Poll<Result<(),Result<Arc<T>,()>>> {
        match self.inner.lock(){
            Ok(x) => {
                let a = &*x;
                match a {
                    Some(x) => {
                        Poll::Ready(match x {
                            Some(x) => {
                                match x {
                                    Some(x) => Err(Ok(x.clone())),
                                    None => Err(Err(()))
                                }
                            }
                            None => Ok(())
                        })
                    }
                    None => Poll::Pending
                }
            }
            Err(_) => Poll::Ready(Err(Err(())))
        }
    }
}

#[allow(unused)]
pub struct GenericRor<T> {
    dummy: T
}

impl <T> Unpin for GenericRor <T> {}

impl <T> GenericRor<T> {
    pub fn new(item:T) -> GenericRor<T>{
        return GenericRor{
            dummy: item
        }
    }
}

impl <T> Future for GenericRor<T> {
    type Output = Result<(),Result<Arc<T>,()>>;

    fn poll (self: Pin<&mut Self>,_cx: &mut Context<'_>) -> Poll<Result<(),Result<Arc<T>,()>>>{
        return Poll::Ready(Err(Err(())));
    }
}

pub struct GenericFut <T> {
    inner: Arc<Mutex<Option<Arc<T>>>>
}

impl <T> GenericFut <T> {
    pub fn new(item: Arc<Mutex<Option<Arc<T>>>>) -> GenericFut<T> {
        GenericFut {
            inner: item
        }
    }
}

impl <T> Unpin for GenericFut <T> {}

impl <T> Future for GenericFut<T> {
    type Output = Result<Arc<T>,()> ;
    fn poll(self: Pin<&mut Self>,_cx: &mut Context<'_>) -> Poll<Result<Arc<T>,()>> {
        match self.inner.lock() {
            Ok(x) => {
                match &*x {
                    Some(i) => Poll::Ready(Ok(i.clone())),
                    None => Poll::Pending
                }
            }
            Err(_) => Poll::Ready(Err(()))
        }
    }
}
/*
pub struct FutureMime<T>
where T: Future {
    inner : Pin<T>
}

impl <T: Future> FutureMime<T> {
    pub fn new(item:Pin<T>) -> Self {
        FutureMime{
            inner: item
        }
    }
}

impl <T: Future> Unpin for FutureMime<T> {}

impl <T: Future> Future for FutureMime<T> {
    type Output = T::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        return self.inner.poll();
    }
}

 */

pub struct FutureMime{
    inner: Arc<Mutex<Option<Result<(),()>>>>
}

impl FutureMime {
    pub fn new(item: Arc<Mutex<Option<Result<(),()>>>>) -> Self{
        FutureMime{
            inner: item
        }
    }
}

impl Unpin for FutureMime {}

impl Future for FutureMime {
    type Output = Result<(),()>;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.inner.lock() {
            Ok(t) => {
                match &*t {
                    Some(x) => Poll::Ready(match x {
                        Ok(_) => Ok(()),
                        Err(_) => Err(())
                    }),
                    None => Poll::Pending
                }
            },
            Err(_) => Poll::Ready(Err(()))
        }
    }
}