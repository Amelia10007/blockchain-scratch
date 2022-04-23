use crate::async_net::{Client, Publisher, Server, Subscriber};
use crate::{Service, Topic};
use async_trait::async_trait;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;
use tokio::sync::oneshot::Sender;
use tokio::task::{JoinError, JoinHandle};
use zeromq::{
    PubSocket, RepSocket, ReqSocket, Socket, SocketRecv, SocketSend, SubSocket, ZmqError,
};

pub struct TopicPublisher<T> {
    socket: PubSocket,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Topic> TopicPublisher<T> {
    pub async fn connect() -> Result<Self, NetError> {
        let mut socket = PubSocket::new();
        socket.connect(&pub_endpoint_name::<T>()).await?;

        let publisher = Self {
            socket,
            _phantom: PhantomData,
        };
        Ok(publisher)
    }
}

#[async_trait]
impl<T: Topic> Publisher<T> for TopicPublisher<T> {
    type Error = NetError;

    async fn publish(&mut self, topic: &T::Pub) -> Result<(), Self::Error> {
        let raw = bincode::serialize(topic)?;
        self.socket.send(raw.into()).await?;
        Ok(())
    }
}

pub struct TopicSubscriber<T> {
    socket: SubSocket,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Topic> TopicSubscriber<T> {
    pub async fn connect() -> Result<Self, NetError> {
        let mut socket = SubSocket::new();
        socket.connect(&sub_endpoint_name::<T>()).await?;
        socket.subscribe("").await?;

        let subscriber = Self {
            socket,
            _phantom: PhantomData,
        };
        Ok(subscriber)
    }
}

#[async_trait]
impl<T: Topic> Subscriber<T> for TopicSubscriber<T> {
    type Error = NetError;

    async fn recv(&mut self) -> Result<T::Sub, NetError> {
        let msg = self.socket.recv().await?;
        let raw = msg.iter().next().ok_or(NetError::Empty)?;

        let sub = bincode::deserialize(raw)?;
        Ok(sub)
    }
}

pub struct ServiceServer<T> {
    socket: RepSocket,
    _phantom: PhantomData<fn() -> T>,
}

impl<S: Service> ServiceServer<S> {
    pub async fn connect() -> Result<Self, NetError> {
        let mut socket = RepSocket::new();
        socket.connect(&server_endpoint_name::<S>()).await?;

        let server = Self {
            socket,
            _phantom: PhantomData,
        };
        Ok(server)
    }
}

#[async_trait]
impl<S: Service> Server<S> for ServiceServer<S> {
    type Error = NetError;

    async fn serve<F>(&mut self, mut f: F) -> Result<(), Self::Error>
    where
        F: FnMut(S::Req) -> Option<S::Res> + Send,
    {
        let req = self.socket.recv().await?;
        let raw = req.iter().next().ok_or(NetError::Empty)?;

        let req = bincode::deserialize(raw)?;
        let req = f(req).ok_or(NetError::Res)?;

        let raw = bincode::serialize(&req)?;
        self.socket.send(raw.into()).await?;

        Ok(())
    }
}

pub struct ServiceClient<T> {
    socket: ReqSocket,
    _phantom: PhantomData<fn() -> T>,
}

impl<S: Service> ServiceClient<S> {
    pub async fn connect() -> Result<Self, NetError> {
        let mut socket = ReqSocket::new();
        socket.connect(&client_endpoint_name::<S>()).await?;

        let client = Self {
            socket,
            _phantom: PhantomData,
        };
        Ok(client)
    }
}

#[async_trait]
impl<S: Service> Client<S> for ServiceClient<S> {
    type Error = NetError;

    async fn request(&mut self, req: &S::Req) -> Result<S::Res, Self::Error> {
        let raw = bincode::serialize(req)?;
        self.socket.send(raw.into()).await?;

        let res = self.socket.recv().await?;
        let raw = res.iter().next().ok_or(NetError::Empty)?;

        let res = bincode::deserialize(raw)?;

        Ok(res)
    }
}

pub struct TopicProxy<T> {
    frontend: SubSocket,
    backend: PubSocket,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> TopicProxy<T> {
    pub async fn bind() -> Result<Self, NetError>
    where
        T: Topic,
    {
        let mut frontend = SubSocket::new();
        frontend.bind(&pub_endpoint_name::<T>()).await?;
        frontend.subscribe("").await?;

        let mut backend = PubSocket::new();
        backend.bind(&sub_endpoint_name::<T>()).await?;

        let proxy = Self {
            frontend,
            backend,
            _phantom: PhantomData,
        };

        Ok(proxy)
    }

    pub fn start(mut self) -> ProxyHandle<T> {
        let (exit_sender, mut exit_receiver) = tokio::sync::oneshot::channel();
        let join_handle = tokio::spawn(async move {
            while let Err(_) = exit_receiver.try_recv() {
                if let Ok(raw) = self.frontend.recv().await {
                    let _res = self.backend.send(raw).await;
                }
            }

            self.frontend.unbind_all().await;
            self.frontend.unsubscribe("").await.ok();
            self.backend.unbind_all().await;
        });

        let proxy_handle = ProxyHandle {
            exit_sender,
            join_handle,
            _phantom: PhantomData,
        };

        proxy_handle
    }
}

pub struct ServiceProxy<S> {
    frontend: RepSocket,
    backend: ReqSocket,
    _phantom: PhantomData<fn() -> S>,
}

impl<S: Service> ServiceProxy<S> {
    pub async fn bind() -> Result<Self, NetError>
    where
        S: Service,
    {
        let mut frontend = RepSocket::new();
        frontend.bind(&client_endpoint_name::<S>()).await?;

        let mut backend = ReqSocket::new();
        backend.bind(&server_endpoint_name::<S>()).await?;

        let proxy = Self {
            frontend,
            backend,
            _phantom: PhantomData,
        };

        Ok(proxy)
    }

    pub fn start(mut self) -> ProxyHandle<S> {
        let (exit_sender, mut exit_receiver) = tokio::sync::oneshot::channel();
        let join_handle = tokio::spawn(async move {
            while let Err(_) = exit_receiver.try_recv() {
                if let Ok(raw) = self.frontend.recv().await {
                    self.backend.send(raw).await.ok();
                }
                if let Ok(raw) = self.backend.recv().await {
                    self.frontend.send(raw).await.ok();
                }
            }

            self.frontend.unbind_all().await;
            self.backend.unbind_all().await;
        });

        let proxy_handle = ProxyHandle {
            exit_sender,
            join_handle,
            _phantom: PhantomData,
        };

        proxy_handle
    }
}

pub struct ProxyHandle<T> {
    exit_sender: Sender<()>,
    join_handle: JoinHandle<()>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> ProxyHandle<T> {
    pub async fn join(self) -> Result<(), NetError> {
        self.exit_sender.send(()).ok();
        self.join_handle.await?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum NetError {
    Zmq(ZmqError),
    Serde(bincode::Error),
    Empty,
    Runtime(JoinError),
    Res,
}

impl From<ZmqError> for NetError {
    fn from(e: ZmqError) -> Self {
        NetError::Zmq(e)
    }
}

impl From<bincode::Error> for NetError {
    fn from(e: bincode::Error) -> Self {
        NetError::Serde(e)
    }
}

impl From<JoinError> for NetError {
    fn from(e: JoinError) -> Self {
        NetError::Runtime(e)
    }
}

impl Display for NetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Zmq(e) => e.fmt(f),
            NetError::Serde(e) => e.fmt(f),
            NetError::Empty => write!(f, "Empty message"),
            NetError::Runtime(e) => e.fmt(f),
            NetError::Res => write!(f, "Failed to create response"),
        }
    }
}

impl std::error::Error for NetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NetError::Zmq(e) => Some(e),
            NetError::Serde(e) => Some(e),
            NetError::Empty => None,
            NetError::Runtime(e) => Some(e),
            NetError::Res => None,
        }
    }
}

fn pub_endpoint_name<T: Topic>() -> String {
    format!("ipc://{}-pub.ipc", T::NAME)
}

fn sub_endpoint_name<T: Topic>() -> String {
    format!("ipc://{}-sub.ipc", T::NAME)
}

fn server_endpoint_name<S: Service>() -> String {
    format!("ipc://{}-srv.ipc", S::NAME)
}

fn client_endpoint_name<S: Service>() -> String {
    format!("ipc://{}-cli.ipc", S::NAME)
}
