use crate::create_topic;
use crate::Topic;
use apply::Apply;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::net::IpAddr;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use warp::Filter;

type Result<T> = std::result::Result<T, NetError>;

create_topic!(NotifyHeartbeat; Heartbeat);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    addr: SocketAddr,
}

impl From<SocketAddr> for Endpoint {
    fn from(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

impl AsRef<SocketAddr> for Endpoint {
    fn as_ref(&self) -> &SocketAddr {
        &self.addr
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    from: Endpoint,
}

impl Heartbeat {
    fn new(from: Endpoint) -> Self {
        Self { from }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatConfig {
    period: Duration,
    timeout: Duration,
}

impl HeartbeatConfig {
    pub fn new(period: Duration, timeout: Duration) -> Self {
        Self { period, timeout }
    }

    pub fn default_config() -> Self {
        Self::new(Duration::from_secs(10), Duration::from_secs(60))
    }
}

#[derive(Debug)]
struct EndpointState {
    endpoint: Endpoint,
    last_heartbeat: Instant,
}

impl EndpointState {
    fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            last_heartbeat: Instant::now(),
        }
    }

    fn update_heartbeat(&mut self) {
        self.last_heartbeat = Instant::now();
    }

    fn is_active(&self, timeout: Duration) -> bool {
        let duration_from_last_heartbeat = Instant::now() - self.last_heartbeat;
        duration_from_last_heartbeat < timeout
    }
}

pub struct Entrance {
    endpoints: Arc<Mutex<Vec<EndpointState>>>,
    config: EntranceConfig,
}

impl Entrance {
    pub fn new(config: EntranceConfig) -> Self {
        Self {
            endpoints: Arc::new(Mutex::new(vec![])),
            config,
        }
    }

    pub async fn start(self) {
        let endpoints = self.endpoints.clone();

        let service = warp::path("blockchain-net-connector")
            .and(warp::query::query())
            .map(move |query: HashMap<String, String>| {
                let mut lock = endpoints.lock().expect("Lock failure");
                let mut endpoints = lock.iter().map(|state| state.endpoint).collect::<Vec<_>>();

                let res = if let Some(Ok(addr)) = query.get("addr").map(|s| SocketAddr::from_str(s))
                {
                    let endpoint = Endpoint::from(addr);
                    // Add endpoints
                    lock.push(EndpointState::new(endpoint));

                    // Search neighbors
                    endpoints.sort_by_cached_key(|&ep| Self::distance(endpoint, ep));

                    let neighbors = endpoints
                        .into_iter()
                        .take(self.config.connection_count)
                        .chain(std::iter::once(self.config.entrance_endpoint))
                        .collect::<Vec<_>>();
                    serde_json::to_string(&neighbors).unwrap_or_default()
                } else {
                    String::new()
                };

                res
            });

        warp::serve(service)
            .run(self.config.entrance_endpoint.as_ref().clone())
            .await;
        // endpoints data must be alive until server running
        drop(self);
    }

    fn request_neighbors(entrance: Endpoint, my: Endpoint) -> Result<Vec<Endpoint>> {
        let url = format!(
            "http:///{}/blockchain-net-connector?{}={}",
            entrance.as_ref().to_string(),
            "addr",
            my.as_ref().to_string()
        );
        let res = reqwest::blocking::get(url)?;
        let bytes = res.bytes()?;
        let neighbors = serde_json::from_slice(&bytes)?;
        Ok(neighbors)
    }

    fn distance(ep1: Endpoint, ep2: Endpoint) -> impl Copy + Ord {
        let ep1 = ep1.as_ref();
        let ep2 = ep2.as_ref();

        let port_distance = (ep1.port() as i32 - ep2.port() as i32).apply(|d| d * d);

        if let (IpAddr::V4(ep1), IpAddr::V4(ep2)) = (ep1.ip(), ep2.ip()) {
            let addr_distance = ep1
                .octets()
                .iter()
                .zip(ep2.octets().iter())
                .map(|(&o1, &o2)| o1 as i32 - o2 as i32)
                .map(|x| x * x)
                .sum::<i32>();

            port_distance + addr_distance
        } else {
            port_distance
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntranceConfig {
    entrance_endpoint: Endpoint,
    connection_count: usize,
}

impl EntranceConfig {
    pub fn new(entrance_endpoint: Endpoint, connection_count: usize) -> Self {
        Self {
            entrance_endpoint,
            connection_count,
        }
    }
}

#[derive(Debug)]
struct BackendInner {
    endpoint: Endpoint,
    neighbors: Mutex<Vec<EndpointState>>,
    topics_map: Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>>,
    join_handle: Option<BackendJoinHandle>,
}

impl BackendInner {
    fn bind(endpoint: Endpoint, neighbors: Vec<Endpoint>) -> Result<Self> {
        let listener = TcpListener::bind(endpoint.as_ref())?;
        listener.set_nonblocking(true)?;

        let neighbors = neighbors.into_iter().map(EndpointState::new).collect();
        let topics_map = Arc::new(Mutex::new(HashMap::new()));

        let join_handle = Self::start_listening(listener, topics_map.clone());

        let backend = Self {
            endpoint,
            neighbors: Mutex::new(neighbors),
            topics_map,
            join_handle: Some(join_handle),
        };

        Ok(backend)
    }

    fn publish<T: Topic>(&self, topic: &T::Pub) -> Result<()> {
        let buf = Self::serialize_to_bytes::<T>(topic)?;
        let neighbors = self.neighbors.lock().expect("Lock failure");

        for neighbor in neighbors.iter() {
            let addr = neighbor.endpoint.as_ref();
            if let Ok(mut stream) = TcpStream::connect(addr) {
                stream.write_all(&buf).ok();
            }
        }

        Ok(())
    }

    fn start_listening(
        listener: TcpListener,
        topics: Arc<Mutex<HashMap<String, VecDeque<Vec<u8>>>>>,
    ) -> BackendJoinHandle {
        let (terminate_sender, terminate_receiver) = std::sync::mpsc::channel();

        let join_handle = std::thread::spawn(move || {
            while let Err(_) = terminate_receiver.try_recv() {
                match listener.accept().map(|(stream, _)| stream) {
                    Ok(mut s) => {
                        let mut buf = vec![];
                        if let Ok(_) = s.read_to_end(&mut buf) {
                            if let Ok((name, topic_bytes)) = Self::deserialize_to_tuple(&buf) {
                                topics
                                    .lock()
                                    .expect("Lock failure")
                                    .entry(name)
                                    .or_insert(VecDeque::new())
                                    .push_back(topic_bytes);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        break;
                    }
                }
            }
        });

        BackendJoinHandle {
            terminate_sender: Mutex::new(terminate_sender),
            join_handle: Some(join_handle),
        }
    }

    fn serialize_to_tuple<T: Topic>(data: &T::Pub) -> Result<(&'static str, Vec<u8>)> {
        let bytes = bincode::serialize(data)?;
        Ok((T::NAME, bytes))
    }

    fn serialize_to_bytes<T: Topic>(data: &T::Pub) -> Result<Vec<u8>> {
        let tuple = Self::serialize_to_tuple::<T>(data)?;
        let bytes = bincode::serialize(&tuple)?;
        Ok(bytes)
    }

    fn deserialize_to_tuple(data: &[u8]) -> Result<(String, Vec<u8>)> {
        let tuple = bincode::deserialize(data)?;
        Ok(tuple)
    }
}

impl Drop for BackendInner {
    fn drop(&mut self) {
        if let Some(handle) = self.join_handle.take() {
            drop(handle)
        }
    }
}

#[derive(Debug)]
pub struct BackendJoinHandle {
    terminate_sender: Mutex<Sender<()>>,
    join_handle: Option<JoinHandle<()>>,
}

impl Drop for BackendJoinHandle {
    fn drop(&mut self) {
        self.terminate_sender.lock().map(|s| s.send(()).ok()).ok();
        self.join_handle.take().map(|h| h.join().ok());
    }
}

pub struct Backend {
    inner: Arc<BackendInner>,
    /// Close heartbeat publisher thread on drop
    _join_handle_heartbeat_publisher: BackendJoinHandle,
    /// Close heartbeat subscriber thread on drop
    _join_handle_heartbeat_subscriber: BackendJoinHandle,
}

impl Backend {
    pub fn bind(
        entrance: Endpoint,
        my: Endpoint,
        heartbeat_config: HeartbeatConfig,
    ) -> Result<Self> {
        let neighbors = Entrance::request_neighbors(entrance, my)?;
        let inner = BackendInner::bind(my, neighbors)?;
        let inner = Arc::new(inner);

        let join_handle_heartbeat_publisher =
            Publisher::from_backend_inner(inner.clone()).start_heartbeat(heartbeat_config.period);
        let join_handle_heartbeat_subscriber = Subscriber::from_backend_inner(inner.clone())
            .start_heartbeat_subscription(heartbeat_config.timeout, heartbeat_config.period);

        let backend = Self {
            inner,
            _join_handle_heartbeat_publisher: join_handle_heartbeat_publisher,
            _join_handle_heartbeat_subscriber: join_handle_heartbeat_subscriber,
        };
        Ok(backend)
    }

    fn inner(&self) -> Arc<BackendInner> {
        self.inner.clone()
    }
}

pub struct Publisher<T: Topic> {
    inner: Arc<BackendInner>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Topic> Publisher<T> {
    pub fn new(backend: &Backend) -> Self {
        Self::from_backend_inner(backend.inner())
    }

    pub fn publish(&self, topic: &T::Pub) -> Result<()> {
        self.inner.publish::<T>(topic)
    }

    fn from_backend_inner(inner: Arc<BackendInner>) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl Publisher<NotifyHeartbeat> {
    pub fn start_heartbeat(self, period: Duration) -> BackendJoinHandle {
        let (terminate_sender, terminate_receiver) = std::sync::mpsc::channel();

        let join_handle = std::thread::spawn(move || {
            while let Err(_) = terminate_receiver.try_recv() {
                let heartbeat = Heartbeat::new(self.inner.endpoint);
                self.inner.publish::<NotifyHeartbeat>(&heartbeat).ok();
                println!("Send heartbeat");
                std::thread::sleep(period);
            }
        });

        BackendJoinHandle {
            terminate_sender: Mutex::new(terminate_sender),
            join_handle: Some(join_handle),
        }
    }
}

pub struct Subscriber<T: Topic> {
    inner: Arc<BackendInner>,
    _phantom: PhantomData<fn() -> T>,
}

impl<T: Topic> Subscriber<T> {
    pub fn new(backend: &Backend) -> Self {
        Self::from_backend_inner(backend.inner())
    }

    pub fn try_recv(&self) -> Result<T::Sub> {
        let mut map = self.inner.topics_map.lock().expect("Lock failure");
        let queue = map.get_mut(T::NAME).ok_or(NetError::NoMessage)?;
        let bytes = queue.pop_front().ok_or(NetError::NoMessage)?;
        let topic = bincode::deserialize(&bytes)?;

        Ok(topic)
    }

    fn from_backend_inner(inner: Arc<BackendInner>) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }
}

impl Subscriber<NotifyHeartbeat> {
    pub fn start_heartbeat_subscription(
        self,
        timeout: Duration,
        period: Duration,
    ) -> BackendJoinHandle {
        let (terminate_sender, terminate_receiver) = std::sync::mpsc::channel();

        let join_handle = std::thread::spawn(move || {
            while let Err(_) = terminate_receiver.try_recv() {
                // Pop all received heartbeats
                while let Ok(heartbeat) = self.try_recv() {
                    println!("Heartbeat from {}", heartbeat.from.addr);
                    // Update heartbeat reception timestamp
                    let mut neighbors = self.inner.neighbors.lock().expect("Lock failure");
                    match neighbors
                        .iter_mut()
                        .find(|neighbor| neighbor.endpoint == heartbeat.from)
                    {
                        Some(neighbor) => neighbor.update_heartbeat(),
                        // Add newcomer as a neighbor.
                        // The newcomer sends heartbeat to me
                        // because the entrance thinks me as a neighbor of the newcomer.
                        None => neighbors.push(EndpointState::new(heartbeat.from)),
                    }
                }
                // Scan, then remove inactive endpoints
                let mut neighbors = self.inner.neighbors.lock().expect("Lock failure");
                neighbors.iter().for_each(|state| {
                    println!(
                        "Endpoint {} active: {}",
                        state.endpoint.addr,
                        state.is_active(timeout)
                    )
                });
                neighbors.retain(|state| state.is_active(timeout));
                std::thread::sleep(period);
            }
        });

        BackendJoinHandle {
            terminate_sender: Mutex::new(terminate_sender),
            join_handle: Some(join_handle),
        }
    }
}

#[derive(Debug)]
pub enum NetError {
    IO(std::io::Error),
    Serde(bincode::Error),
    Json(serde_json::Error),
    EntranceConnection(reqwest::Error),
    NoMessage,
}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        NetError::IO(e)
    }
}

impl From<bincode::Error> for NetError {
    fn from(e: bincode::Error) -> Self {
        NetError::Serde(e)
    }
}

impl From<serde_json::Error> for NetError {
    fn from(e: serde_json::Error) -> Self {
        NetError::Json(e)
    }
}

impl From<reqwest::Error> for NetError {
    fn from(e: reqwest::Error) -> Self {
        NetError::EntranceConnection(e)
    }
}
