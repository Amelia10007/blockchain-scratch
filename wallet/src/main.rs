use blockchain_core::{Address, Coin, Transaction, Transfer, Transition};
use blockchain_net::async_net::{Publisher, Subscriber};
use blockchain_net::impl_zeromq::{TopicPublisher, TopicSubscriber};
use blockchain_net::topic::{CreateTransaction, RequestUtxoByAddress, RespondUtxoByAddress};
use clap::Parser;

#[derive(Debug, Parser)]
struct BcWalletArgs {
    /// File path to secret address
    #[clap(short, long)]
    address: String,

    /// Coin sending destination.
    /// If not specified, bcwallet only display your UTXO.
    #[clap(short, long)]
    destination: Option<Address>,

    /// How much send coin
    /// If not specified, bcwallet only display your UTXO.
    #[clap(short, long)]
    quantity: Option<Coin>,

    /// Fee to paid for miner.
    #[clap(short, long)]
    fee: Option<Coin>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = BcWalletArgs::parse();

    let secret_address = bcaddr::read_address(args.address)?;
    let address = secret_address.to_public_address();

    let mut utxo_requester = TopicPublisher::<RequestUtxoByAddress>::connect().await?;
    let mut utxo_subscriber = TopicSubscriber::<RespondUtxoByAddress>::connect().await?;

    // Request UTXO
    utxo_requester.publish(&address).await?;
    // Wait for UTXO response
    let utxos = utxo_subscriber.recv().await?;
    let utxos = utxos
        .into_iter()
        .filter_map(|tx| tx.verify().ok())
        .collect::<Vec<_>>();

    println!("UTXO:");
    for utxo in utxos.iter() {
        println!("{}", utxo);
    }

    let (dest, send_qty, fee_qty) = match (args.destination, args.quantity, args.fee) {
        (Some(d), Some(q), Some(f)) => (d, q, f),
        _ => return Ok(()),
    };

    let utxo_qty = utxos.iter().map(Transition::quantity).sum::<Coin>();
    let change_qty = if send_qty <= utxo_qty - fee_qty {
        utxo_qty - send_qty - fee_qty
    } else {
        println!(
            "You offer sending {} coin, but your UTXO has only {} coin in total.",
            send_qty, utxo_qty
        );
        return Ok(());
    };

    let transfer = Transfer::offer(&secret_address, dest, send_qty);
    let change = Transfer::offer(&secret_address, address, change_qty);

    let transaction =
        Transaction::offer(&secret_address, utxos, vec![transfer, change]).verify_transaction()?;

    let mut transaction_publisher = TopicPublisher::<CreateTransaction>::connect().await?;
    transaction_publisher.publish(&transaction).await?;

    println!("Notified transaction");

    Ok(())
}
