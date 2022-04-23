use blockchain_net::async_net::Client;
use blockchain_net::impl_zeromq::ServiceClient;
use blockchain_net::service::QueryExample;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating client...");
    let mut client = ServiceClient::<QueryExample>::connect().await?;

    for i in 0..10 {
        println!("Sending request: {}", i);
        match client.request(&i).await {
            Ok(res) => println!("Response: {}", res),
            Err(e) => println!("Failed: {}", e),
        }
    }

    Ok(())
}
