use blockchain_net::async_net::Subscriber;
use blockchain_net::impl_zeromq::TopicSubscriber;
use blockchain_net::topic::PubsubExample;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating subscriber...");
    let mut subscriber = TopicSubscriber::<PubsubExample>::connect().await?;
    println!("Done");

    loop {
        println!("Waiting an address...");
        match subscriber.recv().await {
            Ok(t) => {
                println!("Received: {}", t)
            }
            Err(e) => {
                println!("{}", e);
                break;
            }
        }
        println!("Successfully subscribed");
    }

    Ok(())
}
