use crate::{Service, Topic};
use async_trait::async_trait;

#[async_trait]
pub trait Publisher<T: Topic> {
    type Error: Send + Sync;

    /// Broadcast topic
    async fn publish(&mut self, topic: &T::Pub) -> Result<(), Self::Error>;
}

#[async_trait]
pub trait Subscriber<T: Topic> {
    type Error;

    /// Wait a topic from any publisher
    async fn recv(&mut self) -> Result<T::Sub, Self::Error>;
}

#[async_trait]
pub trait Server<S: Service> {
    type Error;

    async fn serve<F>(&mut self, f: F) -> Result<(), Self::Error>
    where
        F: FnMut(S::Req) -> Option<S::Res> + Send;
}

#[async_trait]
pub trait Client<S: Service> {
    type Error;

    async fn request(&mut self, req: &S::Req) -> Result<S::Res, Self::Error>;
}
