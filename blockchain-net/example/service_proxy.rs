use blockchain_net::impl_zeromq::ServiceProxy;
use blockchain_net::service::QueryExample;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating proxy...");
    let proxy = ServiceProxy::<QueryExample>::bind().await?;

    println!("Running proxy...");
    let handle = proxy.start();

    // Wait enter key
    {
        std::io::stdin().read_line(&mut String::new()).ok();
    }

    println!("Shutdown proxy...");
    // Required for graceful shutdown
    handle.join().await?;

    Ok(())
}
