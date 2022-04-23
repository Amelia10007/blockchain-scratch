use blockchain_net::async_net::Server;
use blockchain_net::impl_zeromq::ServiceServer;
use blockchain_net::service::QueryExample;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating server...");
    let mut server = ServiceServer::<QueryExample>::connect().await?;

    let prefix = "wowwow";

    loop {
        println!("Waiting request...");
        match server
            .serve(|req| Some(format!("{}-{}", prefix, req)))
            .await
        {
            Ok(_) => println!("Successfully served"),
            Err(e) => {
                println!("{}", e);
                break;
            }
        }
    }

    Ok(())
}
