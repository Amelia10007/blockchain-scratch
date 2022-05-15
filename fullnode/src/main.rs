use anyhow::Result;
use blockchain_core::block::block_coin_generation_rule;
use blockchain_core::digest::BlockDigest;
use blockchain_core::ledger::{Ledger, LedgerError};
use blockchain_core::{Block, BlockHeight, BlockSource, SecretAddress, VerifiedBlock, Yet};
use blockchain_core::{Difficulty, Transaction, UnverifiedBlock, Verified};
use blockchain_net::async_net::{Publisher, Subscriber};
use blockchain_net::impl_zeromq::{TopicPublisher, TopicSubscriber};
use blockchain_net::topic::{
    CreateTransaction, NotifyBlock, NotifyBlockHeight, RequestUtxoByAddress, RespondUtxoByAddress,
};
use clap::Parser;
use log::{error, info, warn};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;

const DIFFICULTY: Difficulty = Difficulty::new(10);

fn verify_block_after_mining(
    block: Block<Verified, Yet, Yet, Yet, Yet, Yet>,
    ledger: &Ledger,
) -> Result<VerifiedBlock> {
    let block = block
        .verify_transaction_relation(block_coin_generation_rule)
        .and_then(|b| b.verify_difficulty(&DIFFICULTY))
        .and_then(|b| b.verify_digest())?;
    let block = ledger.verify_block(block)?;

    Ok(block)
}

fn verify_block(block: UnverifiedBlock, ledger: &Ledger) -> Result<VerifiedBlock> {
    let block = block.verify_transaction_itself()?;
    let block = verify_block_after_mining(block, ledger)?;
    Ok(block)
}

fn block_subscription_event(block: UnverifiedBlock, ledger: Arc<Mutex<Ledger>>) -> Result<()> {
    let mut ledger = ledger.lock().expect("Lock failure");
    let block = verify_block(block, &ledger)?;

    match ledger.entry(block) {
        Ok(_) => Ok(()),
        // This event catches a block published from this node.
        // So ignore block duplication error, which occurs everytime on block publication.
        Err(LedgerError::DuplicatedBlock) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn spawn_transaction_subscriber(
    mut subscriber: TopicSubscriber<CreateTransaction>,
    incoming_transactions: Arc<Mutex<Vec<Transaction<Verified, Verified>>>>,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        loop {
            match subscriber.recv().await {
                Ok(transaction) => {
                    info!("Received a transaction.");
                    match transaction.verify() {
                        Ok(transaction) => {
                            info!("Verified the received transaction.");
                            let mut incoming_transactions =
                                incoming_transactions.lock().expect("Lock failure");
                            incoming_transactions.push(transaction);
                            incoming_transactions.sort_by_key(Transaction::timestamp);
                            info!("Verified transaction was queued to incoming transactions.");
                        }
                        Err(e) => error!("Error during transaction verification. {}", e),
                    }
                }
                Err(e) => error!("Error during subscribing transaction. {}", e),
            }
        }
    })
}

fn spawn_block_subscriber(
    mut subscriber: TopicSubscriber<NotifyBlock>,
    ledger: Arc<Mutex<Ledger>>,
    incoming_transactions: Arc<Mutex<Vec<Transaction<Verified, Verified>>>>,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        loop {
            match subscriber.recv().await {
                Ok(block) => {
                    info!(
                        "Received block. Height: {}, Digest: {}",
                        block.height(),
                        hex::encode(block.digest())
                    );
                    match block_subscription_event(block, ledger.clone()) {
                        Ok(_) => {
                            // Clear incoming transaction, since they are verified and added to new block
                            incoming_transactions.lock().expect("Lock failure").clear();
                            info!("Successfully append the received block to ledger")
                        }
                        Err(e) => warn!("Deny incoming block. {}", e),
                    }
                }
                Err(e) => error!("Error during subscribing block. {}", e),
            }
        }
    })
}

fn spawn_block_height_publisher(
    mut height_publisher: TopicPublisher<NotifyBlockHeight>,
    ledger: Arc<Mutex<Ledger>>,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        loop {
            let height = ledger
                .lock()
                .expect("Lock failure")
                .search_latest_block()
                .map(Block::height)
                .unwrap_or(BlockHeight::genesis());

            info!("Publishing local chain height: {}...", height);

            match height_publisher.publish(&height).await {
                Ok(()) => {}
                Err(e) => error!("Error during publishing local chain height: {}", e),
            }

            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    })
}

fn spawn_block_height_subscriber(
    mut height_subscriber: TopicSubscriber<NotifyBlockHeight>,
    publish_sender: Sender<VerifiedBlock>,
    ledger: Arc<Mutex<Ledger>>,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        loop {
            match height_subscriber.recv().await {
                Ok(other_node_height) => {
                    // Longest chain's height
                    let local_block_height =
                        match ledger.lock().expect("Lock failure").search_latest_block() {
                            Some(block) => block.height(),
                            None => continue,
                        };
                    // If this ledger has longer chain than other,
                    // publish the longest chain of local ledger
                    if other_node_height >= local_block_height {
                        continue;
                    }

                    info!("Another node has shorter chain than this node's. Publishing the longest chain of this node...");

                    let mut current_height = BlockHeight::genesis();
                    loop {
                        // Get block at current target height
                        let block = ledger
                            .lock()
                            .expect("Lock failure")
                            .search_latest_chain()
                            .find(|block| block.height() == current_height)
                            .cloned();
                        // Publish
                        match block {
                            Some(block) => match publish_sender.send(block).await {
                                Ok(_) => info!("Published block {}", current_height),
                                Err(e) => error!("Error during publishing block: {}", e),
                            },
                            None => break,
                        }
                        current_height = current_height.next();
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
                Err(e) => error!("Error during subscribing block height. {}", e),
            }
        }
    })
}

fn spawn_mining_join_handle(
    incoming_transactions: Arc<Mutex<Vec<Transaction<Verified, Verified>>>>,
    publish_sender: Sender<VerifiedBlock>,
    ledger: Arc<Mutex<Ledger>>,
    secret_address: SecretAddress,
    mine_genesis_block: bool,
) -> JoinHandle<()> {
    tokio::task::spawn(async move {
        loop {
            let transactions = incoming_transactions.lock().expect("Lock failure").to_vec();
            let (next_height, previous_digest) =
                match ledger.lock().expect("Lock failure").search_latest_block() {
                    Some(block) => (block.height().next(), block.digest().clone()),
                    None => (BlockHeight::genesis(), BlockDigest::digest(&[])),
                };

            // Check whether mine genesis block
            if next_height == BlockHeight::genesis() && !mine_genesis_block {
                warn!("Mining genesis block is disabled. Wait for genesis block from other nodes.");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }

            if next_height > BlockHeight::genesis() && transactions.is_empty() {
                warn!("No transaction come yet. Wait for transactions...");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }

            let block_src = BlockSource::new(
                next_height,
                transactions,
                previous_digest,
                DIFFICULTY.clone(),
                rand::thread_rng().gen(),
                &secret_address,
                blockchain_core::block::block_coin_generation_rule,
            );

            if let Ok(block_src) = block_src {
                if let Ok(block) = block_src.try_into_block() {
                    let res = {
                        let ledger = ledger.lock().expect("Lock failure");
                        verify_block_after_mining(block, &ledger)
                    };
                    match res {
                        Ok(block) => {
                            info!(
                                "Found new block. Height: {}, Digest: {}",
                                block.height(),
                                hex::encode(block.digest())
                            );

                            // Publish found block
                            match publish_sender.send(block.clone()).await {
                                Ok(_) => info!("Published the latest block."),
                                Err(e) => error!("Error during publishing a block. {}", e),
                            }

                            // Clear incoming transaction, since they are verified and added to new block
                            incoming_transactions.lock().expect("Lock failure").clear();

                            // Append new block to ledger
                            let mut ledger = ledger.lock().expect("Lock failure");
                            match ledger.entry(block.clone()) {
                                Ok(_) => info!("Successfully appended new block."),
                                Err(e) => error!("Error during adding new block. {}", e),
                            }
                        }
                        Err(e) => {
                            // Clear all incoming transactions since they contains invalid transactions,
                            // which may prevent next verification process.
                            warn!("Block verification failed: {}", e);
                            warn!("Clear incoming transactions.");
                            incoming_transactions.lock().expect("Lock failure").clear();
                        }
                    }
                }
            }

            // Wait next mining
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
}

fn spawn_block_publisher(
    mut publisher: TopicPublisher<NotifyBlock>,
    mut receiver: Receiver<VerifiedBlock>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(block) = receiver.recv().await {
            match publisher.publish(&block).await {
                Ok(()) => {}
                Err(e) => error!("Error during publishing block: {}", e),
            }
        }
        warn!("Block publisher thread finished. Inner block publication functionality may have finished");
    })
}

fn spawn_utxo_pubsub(
    mut publisher: TopicPublisher<RespondUtxoByAddress>,
    mut subscriber: TopicSubscriber<RequestUtxoByAddress>,
    ledger: Arc<Mutex<Ledger>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let address = match subscriber.recv().await {
                Ok(address) => address,
                Err(e) => {
                    error!("Error during receiving UTXO request: {}", e);
                    continue;
                }
            };

            // List UTXO of requested address in the longest chain
            let utxos = {
                let ledger = ledger.lock().expect("Lock failure");
                match ledger.search_latest_block() {
                    Some(latest_block) => ledger.build_utxos(latest_block.digest(), &address),
                    None => vec![],
                }
            };

            match publisher.publish(&utxos).await {
                Ok(_) => info!("Publish {} UTXO of {}.", utxos.len(), address),
                Err(e) => error!("Error during publishing UTXO: {}", e),
            }
        }
    })
}

#[derive(Debug, Parser)]
struct FullnodeArgs {
    /// Address file path
    #[clap(long)]
    address: String,

    /// Enable when mine genesis block. Otherwise, download genesis block from other nodes.
    #[clap(long)]
    mine_genesis_block: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let arg = FullnodeArgs::parse();

    info!("Initializing blockchain full node...");

    let secret_address = bcaddr::read_address(&arg.address)?;
    info!("Loaded self address from {}.", &arg.address);

    let incoming_transactions = Arc::new(Mutex::new(vec![]));
    let ledger = Arc::new(Mutex::new(Ledger::new()));
    info!("Spawning connection functionality...");

    let transaction_subscriber = TopicSubscriber::<CreateTransaction>::connect().await?;
    let block_subscriber = TopicSubscriber::<NotifyBlock>::connect().await?;
    let block_publisher = TopicPublisher::<NotifyBlock>::connect().await?;
    let block_height_publisher = TopicPublisher::<NotifyBlockHeight>::connect().await?;
    let block_height_subscriber = TopicSubscriber::<NotifyBlockHeight>::connect().await?;
    let utxo_publisher = TopicPublisher::<RespondUtxoByAddress>::connect().await?;
    let utxo_subscriber = TopicSubscriber::<RequestUtxoByAddress>::connect().await?;

    let (block_publish_sender, block_publish_receiver) = tokio::sync::mpsc::channel(10);

    info!("Spawning threads...");

    let transaction_subsctiber_join_handle =
        spawn_transaction_subscriber(transaction_subscriber, incoming_transactions.clone());
    let block_subscriber_join_handle = spawn_block_subscriber(
        block_subscriber,
        ledger.clone(),
        incoming_transactions.clone(),
    );
    let block_height_publisher_join_handle =
        spawn_block_height_publisher(block_height_publisher, ledger.clone());
    let block_height_subscriber_join_handle = spawn_block_height_subscriber(
        block_height_subscriber,
        block_publish_sender.clone(),
        ledger.clone(),
    );
    let mining_join_handle = spawn_mining_join_handle(
        incoming_transactions.clone(),
        block_publish_sender,
        ledger.clone(),
        secret_address,
        arg.mine_genesis_block,
    );
    let block_publisher_join_handle =
        spawn_block_publisher(block_publisher, block_publish_receiver);
    let utxo_pubsub_join_handle = spawn_utxo_pubsub(utxo_publisher, utxo_subscriber, ledger);

    info!("Initialization done. A blockchain-fullnode runnning...");

    transaction_subsctiber_join_handle.await?;
    block_subscriber_join_handle.await?;
    block_height_publisher_join_handle.await?;
    block_height_subscriber_join_handle.await?;
    mining_join_handle.await?;
    block_publisher_join_handle.await?;
    utxo_pubsub_join_handle.await?;

    Ok(())
}
