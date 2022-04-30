use blockchain_net::blocking::{Backend, Endpoint, HeartbeatConfig, Subscriber};
use blockchain_net::topic::PubsubExample;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

fn main() {
    let local_addr = std::env::args()
        .nth(1)
        .expect("Provide entrance address:port");
    let local_addr = SocketAddr::from_str(&local_addr).expect("Address:port parse error");
    let local_endpoint = Endpoint::from(local_addr);

    let entrance_addr = std::env::args()
        .nth(2)
        .expect("Provide entrance address:port");
    let entrance_addr = SocketAddr::from_str(&entrance_addr).expect("Address:port parse error");
    let entrance_endpoint = Endpoint::from(entrance_addr);

    let heartbeat_config = HeartbeatConfig::default_config();

    let backend = Backend::bind(entrance_endpoint, local_endpoint, heartbeat_config).unwrap();

    let subscriber = Subscriber::<PubsubExample>::new(&backend);

    loop {
        while let Ok(topic) = subscriber.try_recv() {
            println!("Subscribed: {}", topic);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
