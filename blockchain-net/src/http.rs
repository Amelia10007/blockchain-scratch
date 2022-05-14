use crate::{Service, Topic};
use bytes::Bytes;
use reqwest::blocking::{Client, ClientBuilder, Response};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Read;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{marker::PhantomData, net::SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot::{Receiver, Sender};
use tokio::task::JoinHandle;
use warp::Filter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Endpoint {
    ip: IpAddr,
}

impl Endpoint {
    pub const fn new(ip: IpAddr) -> Self {
        Self { ip }
    }

    const fn topic_port() -> u16 {
        32001
    }

    const fn service_port() -> u16 {
        32002
    }

    pub fn topic_socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, Self::topic_port())
    }

    pub fn service_socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, Self::service_port())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointState {
    Active(Endpoint),
    Questionable(Endpoint, u32),
    Inactive(Endpoint),
}

impl EndpointState {
    fn endpoint(&self) -> Endpoint {
        use EndpointState::*;
        match self {
            Active(e) | Questionable(e, _) | Inactive(e) => *e,
        }
    }

    fn next_ok(&mut self) {
        *self = EndpointState::Active(self.endpoint());
    }

    fn next_err(&mut self) {
        use EndpointState::*;
        let e = self.endpoint();
        *self = match *self {
            Active(_) => Questionable(e, 1),
            Questionable(_, n) if n < 10 => Questionable(e, n + 1),
            Questionable(_, _) => Inactive(e),
            Inactive(e) => Inactive(e),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TopicTransferOwned {
    name: String,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TopicTransfer<'a, 'b> {
    name: &'a str,
    data: &'b [u8],
}

struct TopicBackend {
    topic_queue: Arc<Mutex<VecDeque<TopicTransferOwned>>>,
    neighbors: Arc<Mutex<Vec<EndpointState>>>,
    subscription_join_handle: JoinHandle<()>,
    shutdown_sender: Sender<()>,
}

impl TopicBackend {
    async fn bind(
        endpoint: Endpoint,
        neighbors: Arc<Mutex<Vec<EndpointState>>>,
    ) -> Result<Self, Error> {
        let listener = TcpListener::bind(endpoint.topic_socket()).await?;
        let timeout = Duration::from_secs(10);
        let topic_queue = Arc::new(Mutex::new(VecDeque::new()));
        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let subscription_join_handle = Self::start_subscription(
            listener,
            timeout,
            topic_queue.clone(),
            neighbors.clone(),
            shutdown_receiver,
        );

        let backend = Self {
            topic_queue,
            neighbors,
            subscription_join_handle,
            shutdown_sender,
        };
        Ok(backend)
    }

    async fn send(&self, topic_name: &str, data: &[u8]) {
        let transfer = TopicTransfer {
            name: topic_name,
            data,
        };
        let bytes = bincode::serialize(&transfer).expect("Serialization fail");
        if let Ok(mut neighbors) = self.neighbors.lock() {
            for neighbor in neighbors.iter_mut() {
                let addr = neighbor.endpoint().topic_socket();
                // Update neighbor state
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    if let Ok(_) = stream.write_all(&bytes).await {
                        neighbor.next_ok();
                    } else {
                        neighbor.next_err();
                    }
                } else {
                    neighbor.next_err();
                }
            }
        }
    }

    fn start_subscription(
        listener: TcpListener,
        timeout: Duration,
        topic_queue: Arc<Mutex<VecDeque<TopicTransferOwned>>>,
        neighbors: Arc<Mutex<Vec<EndpointState>>>,
        mut shutdown_receiver: Receiver<()>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Err(_) = shutdown_receiver.try_recv() {
                let (mut stream, remote_addr) =
                    match tokio::time::timeout(timeout, listener.accept()).await {
                        Ok(Ok(tuple)) => tuple,
                        _ => continue,
                    };

                let mut buf = vec![];
                if let Err(_) = stream.read_to_end(&mut buf).await {
                    continue;
                }

                let transfer = match bincode::deserialize::<TopicTransferOwned>(&buf) {
                    Ok(t) => t,
                    Err(_) => continue,
                };

                if let Ok(mut topic_queue) = topic_queue.lock() {
                    topic_queue.push_back(transfer);
                }

                // Update neighbor state
                if let Ok(mut neighbors) = neighbors.lock() {
                    let remote_ip = remote_addr.ip();
                    let endpoint = Endpoint::new(remote_ip);
                    let state = EndpointState::Active(endpoint);
                    match neighbors.iter_mut().find(|n| n.endpoint() == endpoint) {
                        Some(n) => *n = state,
                        None => neighbors.push(state),
                    }
                }
            }
        })
    }
}

pub struct Publisher<T> {
    backend: Arc<TopicBackend>,
    _phantom: PhantomData<fn() -> T>,
}

pub struct Subscriber<T> {
    backend: Arc<TopicBackend>,
    _phantom: PhantomData<fn() -> T>,
}

struct Serve {
    service: &'static str,
    handler: Box<dyn FnMut(&str) -> Option<String> + Send + 'static>,
}

pub struct ServiceBackend {
    servers: Arc<Mutex<Vec<Serve>>>,
    neighbors: Arc<Mutex<Vec<EndpointState>>>,
    shutdown_sender: Sender<()>,
}

impl ServiceBackend {
    async fn start(endpoint: Endpoint, neighbors: Arc<Mutex<Vec<EndpointState>>>) -> Self {
        let servers = Arc::new(Mutex::new(vec![]));
        let shutdown_sender = Self::start_server(endpoint, servers.clone());
        Self {
            servers,
            neighbors,
            shutdown_sender,
        }
    }

    fn start_server(endpoint: Endpoint, servers: Arc<Mutex<Vec<Serve>>>) -> Sender<()> {
        let service = warp::path::param().and(warp::body::bytes()).map(
            move |service_name: String, req: Bytes| {
                let req_string = std::str::from_utf8(&req).unwrap();
                let mut servers = servers.lock().expect("Lock failure");

                for server in servers.iter_mut() {
                    if server.service == service_name {
                        let res = (server.handler)(req_string);
                        if let Some(s) = res {
                            return warp::reply::json(&s);
                        }
                    }
                }

                warp::reply::json(&String::new())
            },
        );

        let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel();
        let (_, server) =
            warp::serve(service).bind_with_graceful_shutdown(endpoint.service_socket(), async {
                shutdown_receiver.await.ok();
            });

        tokio::spawn(server);

        shutdown_sender
    }
}

pub struct Backend {
    topic_backend: Arc<TopicBackend>,
    service_backend: Arc<ServiceBackend>,
    neighbors: Arc<Mutex<Vec<EndpointState>>>,
}

impl Backend {
    pub async fn start(endpoint: Endpoint) -> Result<Self, Error> {
        let neighbors = Arc::new(Mutex::new(vec![]));
        let topic_backend = TopicBackend::bind(endpoint, neighbors.clone()).await?;
        let service_backend = ServiceBackend::start(endpoint, neighbors.clone()).await;
        let backend = Self {
            topic_backend: Arc::new(topic_backend),
            service_backend: Arc::new(service_backend),
            neighbors,
        };
        Ok(backend)
    }
}

pub struct HttpServer<S> {
    _phantom: PhantomData<fn() -> S>,
}

impl<S: Service> HttpServer<S> {
    pub async fn start<F>(addr: impl Into<SocketAddr>, handler: F)
    where
        F: Fn(S::Req) -> Option<S::Res> + Clone + Send + Sync + 'static,
    {
        let service = warp::path(S::NAME)
            .and(warp::body::json::<S::Req>())
            .map(move |req| {
                println!("DEBUG: request: {}", serde_json::to_string(&req).unwrap());
                let req = handler(req);
                req.as_ref()
                    .map(warp::reply::json)
                    .unwrap_or(warp::reply::json(&""))
            });

        warp::serve(service).run(addr).await;
    }
}

#[derive(Debug, Clone)]
pub struct DestinationCollection {
    sockets: Vec<SocketAddr>,
}

impl DestinationCollection {
    pub fn new(sockets: Vec<SocketAddr>) -> Self {
        Self { sockets }
    }
}

pub struct HttpClient<S> {
    destination: DestinationCollection,
    client: Client,
    _phantom: PhantomData<fn() -> S>,
}

impl<S: Service> HttpClient<S> {
    pub fn new(destination: DestinationCollection) -> Result<Self, ClientError> {
        let client = ClientBuilder::new().build()?;
        let httpclient = Self {
            destination,
            client,
            _phantom: PhantomData,
        };
        Ok(httpclient)
    }

    pub fn call(&self, req: &S::Req) -> Result<S::Res, ClientError> {
        let json = serde_json::to_string(req)?;

        for url in self.urls() {
            let req = self.client.get(url).body(json.clone()).build()?;
            if let Ok(res_text) = self.client.execute(req).and_then(Response::text) {
                if let Ok(res) = serde_json::from_str::<S::Res>(&res_text) {
                    return Ok(res);
                }
            }
        }

        Err(ClientError::NoResponse)
    }

    fn urls(&self) -> impl Iterator<Item = Url> + '_ {
        self.destination
            .sockets
            .iter()
            .map(|socket| {
                Url::parse(&format!("http://{}", socket.to_string()))
                    .and_then(|url| url.join(S::NAME))
            })
            .flatten()
    }
}

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

#[derive(Debug)]
pub enum ClientError {
    Serde(serde_json::Error),
    Request(reqwest::Error),
    NoResponse,
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::Serde(e)
    }
}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Request(e)
    }
}
