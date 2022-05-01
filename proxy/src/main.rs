use blockchain_net::impl_zeromq::TopicProxy;
use blockchain_net::topic::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating proxy...");
    let proxy_tx = TopicProxy::<CreateTransaction>::bind().await?;
    let proxy_block = TopicProxy::<NotifyBlock>::bind().await?;
    let proxy_block_height = TopicProxy::<NotifyBlockHeight>::bind().await?;
    let utxo_req = TopicProxy::<RequestUtxoByAddress>::bind().await?;
    let utxo_res = TopicProxy::<RespondUtxoByAddress>::bind().await?;

    println!("Running proxy...");
    let handle_tx = proxy_tx.start();
    let handle_block = proxy_block.start();
    let handle_block_height = proxy_block_height.start();
    let utxo_req = utxo_req.start();
    let utxo_res = utxo_res.start();

    // Wait enter key
    {
        println!("Type enter to shutdown proxy.");
        std::io::stdin().read_line(&mut String::new()).ok();
    }

    println!("Shutdown proxy...");
    // Graceful shutdown
    handle_tx.join().await?;
    handle_block.join().await?;
    handle_block_height.join().await?;
    utxo_req.join().await?;
    utxo_res.join().await?;

    println!("Bye.");
    Ok(())
}
