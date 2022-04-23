use blockchain_net::async_net::Publisher;
use blockchain_net::impl_zeromq::TopicPublisher;
use blockchain_net::topic::PubsubExample;
use std::thread;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating publisher...");
    let mut publisher = TopicPublisher::<PubsubExample>::connect().await?;
    println!("Done");

    for i in 0..10 {
        println!("Publishing data: {:?}...", i);
        if let Err(e) = publisher.publish(&i).await {
            println!("{}", e);
            break;
        }
        println!("Successfully published");

        thread::sleep(Duration::from_secs(1));
    }

    Ok(())
}
