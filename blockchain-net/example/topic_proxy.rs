use blockchain_net::impl_zeromq::TopicProxy;
use blockchain_net::topic::PubsubExample;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating proxy...");
    let proxy = TopicProxy::<PubsubExample>::bind().await?;

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
