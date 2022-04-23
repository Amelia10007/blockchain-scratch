use blockchain_net::impl_zeromq::TopicProxy;
use blockchain_net::topic::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating proxy...");
    let proxy_tx = TopicProxy::<CreateTransaction>::bind().await?;
    let proxy_block = TopicProxy::<NotifyBlock>::bind().await?;

    println!("Running proxy...");
    let handle_tx = proxy_tx.start();
    let handle_block = proxy_block.start();

    // Wait enter key
    {
        println!("Type enter to shutdown proxy.");
        std::io::stdin().read_line(&mut String::new()).ok();
    }

    println!("Shutdown proxy...");
    // Required for graceful shutdown
    handle_tx.join().await?;
    handle_block.join().await?;

    println!("Bye.");
    Ok(())
}
