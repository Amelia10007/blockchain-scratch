use blockchain_net::blocking::{Entrance, EntranceConfig};
use std::net::SocketAddr;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args()
        .nth(1)
        .expect("Provide entrance address:port");
    let addr = SocketAddr::from_str(&addr).expect("Address:port parse error");
    let config = EntranceConfig::new(addr.into(), 5);

    let entrance = Entrance::new(config);

    entrance.start().await;

    Ok(())
}
