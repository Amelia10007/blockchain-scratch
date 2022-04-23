use anyhow::{bail, Result};
use blockchain_core::block::block_coin_generation_rule;
use blockchain_core::digest::BlockDigest;
use blockchain_core::history::{BlockHistory, TransferHistory, TransferState};
use blockchain_core::{Block, BlockHeight, BlockSource, VerifiedBlock, Yet};
use blockchain_core::{Difficulty, Transaction, UnverifiedBlock, Verified};
use blockchain_net::async_net::{Publisher, Subscriber};
use blockchain_net::impl_zeromq::TopicPublisher;
use blockchain_net::topic::CreateTransaction;
use blockchain_net::{impl_zeromq::TopicSubscriber, topic::NotifyBlock};
use clap::Parser;
use log::{error, info};
use rand::Rng;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

const DIFFICULTY: Difficulty = Difficulty::new(14);

fn load_block_history(path: impl AsRef<Path>) -> Result<Vec<UnverifiedBlock>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = vec![];
    reader.read_to_end(&mut buf)?;
    let blocks = bincode::deserialize(&buf)?;
    Ok(blocks)
}

fn save_block_history(path: impl AsRef<Path>, history: &BlockHistory) -> Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    let blocks = history.sorted_blocks_by_height();
    let bytes = bincode::serialize(blocks)?;

    writer.write(&bytes)?;

    Ok(())
}

fn construct_block_history(
    blocks: Vec<UnverifiedBlock>,
) -> Result<(TransferHistory, BlockHistory)> {
    let mut transfer_history = TransferHistory::new();
    let mut block_history = BlockHistory::new();

    for block in blocks.into_iter() {
        let block = verify_block(block, &transfer_history, &block_history)?;
        transfer_history.push_block(&block)?;
        block_history.push_block(block)?;
    }

    Ok((transfer_history, block_history))
}

fn judge_utxo(
    transfer_history: &TransferHistory,
    transaction: &Transaction<Verified, Verified>,
) -> bool {
    transaction
        .inputs()
        .iter()
        .chain(transaction.outputs())
        .map(|i| transfer_history.state_of(i))
        .all(|s| matches!(s, TransferState::Unlisted | TransferState::Unused))
}

fn verify_block(
    block: UnverifiedBlock,
    transfer_history: &TransferHistory,
    block_history: &BlockHistory,
) -> Result<VerifiedBlock> {
    let block = block.verify_transaction_itself()?;
    let block = verify_block_after_mining(block, transfer_history, block_history)?;

    Ok(block)
}

fn verify_block_after_mining(
    block: Block<Verified, Yet, Yet, Yet, Yet, Yet>,
    transfer_history: &TransferHistory,
    block_history: &BlockHistory,
) -> Result<VerifiedBlock> {
    let block = block
        .verify_transaction_relation(block_coin_generation_rule)
        .and_then(|b| b.verify_utxo(|ts| ts.iter().all(|t| judge_utxo(&transfer_history, t))))
        .and_then(|b| b.verify_difficulty(&DIFFICULTY))
        .and_then(|b| b.verify_digest())
        .and_then(|b| {
            b.verify_previous_block(
                |h| block_history.block_at(h).map(Block::digest),
                |h| block_history.block_at(h).map(Block::timestamp),
            )
        })?;

    Ok(block)
}

fn block_subscription_event(
    block: UnverifiedBlock,
    transfer_history: Arc<Mutex<TransferHistory>>,
    block_history: Arc<Mutex<BlockHistory>>,
) -> Result<()> {
    let mut block_history = block_history.lock().expect("Lock failure");
    let mut transfer_history = transfer_history.lock().expect("Lock failure");

    let block = verify_block(block, &transfer_history, &block_history)?;

    if let Some(previous) = block_history.sorted_blocks_by_height().last() {
        if previous.digest() == block.digest() {
            return Ok(());
        }
        if previous.height() >= block.height() {
            bail!(
                "Received block has invalid height. Latest block: {:?} but received block {:?}",
                previous.height(),
                block.height()
            );
        }
    }
    info!("Block verification succeeded.");

    transfer_history.push_block(&block)?;
    info!("Added transfers of the verified block.");
    block_history.push_block(block)?;
    info!("Added a verified block.");

    Ok(())
}

#[derive(Debug, Parser)]
struct FullnodeArgs {
    /// Address file path
    #[clap(short, long)]
    address: String,
    /// History file path
    #[clap(short, long)]
    history: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let arg = FullnodeArgs::parse();

    info!("Initializing blockchain full node...");

    let secret_address = bcaddr::read_address(&arg.address)?;
    info!("Loaded self address from {}.", &arg.address);

    let blocks = load_block_history(&arg.history).unwrap_or(vec![]);
    info!("Loaded block history from {}.", &arg.history);
    let (transfer_history, block_history) = construct_block_history(blocks)?;
    info!("Successfully constructed transfer/block history.");

    let incoming_transactions = Arc::new(Mutex::new(vec![]));
    let transfer_history = Arc::new(Mutex::new(transfer_history));
    let block_history = Arc::new(Mutex::new(block_history));

    info!("Spawning connection functionality...");

    let mut transaction_subscriber = TopicSubscriber::<CreateTransaction>::connect().await?;
    let mut block_subscriber = TopicSubscriber::<NotifyBlock>::connect().await?;
    let mut block_publisher = TopicPublisher::<NotifyBlock>::connect().await?;

    info!("Sparning threads...");

    let incoming_transactions_clone = incoming_transactions.clone();
    let transaction_subsctiber_join_handle = tokio::task::spawn(async move {
        loop {
            match transaction_subscriber.recv().await {
                Ok(transaction) => {
                    info!("Received a transaction.");
                    match transaction.verify() {
                        Ok(transaction) => {
                            info!("Verified the received transaction.");
                            let mut incoming_transactions =
                                incoming_transactions_clone.lock().expect("Lock failure");
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
    });

    let block_history_clone = block_history.clone();
    let transfer_history_clone = transfer_history.clone();
    let block_subscriber_join_handle = tokio::task::spawn(async move {
        loop {
            match block_subscriber.recv().await {
                Ok(block) => {
                    let digest = hex::encode(block.digest());
                    match block_subscription_event(
                        block,
                        transfer_history_clone.clone(),
                        block_history_clone.clone(),
                    ) {
                        Ok(_) => info!("Received block. Digest: {}", digest),
                        Err(e) => error!("Error during adding a block. {}", e),
                    }
                }
                Err(e) => error!("Error during subscribing block. {}", e),
            }
        }
    });

    let block_publisher_join_handle = tokio::task::spawn(async move {
        // loop {
        //     let latest_block = block_history_clone
        //         .lock()
        //         .expect("Lock failure")
        //         .sorted_blocks_by_height()
        //         .last()
        //         .cloned();
        //     match latest_block {
        //         Some(latest_block) => {
        //             match block_publisher.publish(&latest_block).await {
        //                 Ok(_) => info!("Published the latest block"),
        //                 Err(e) => {} // error!("Error during publishing a block. {}", e),
        //             }
        //         }
        //         None => {}
        //     }
        //     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        // }
    });

    let mining_join_handle = tokio::task::spawn(async move {
        let history_path = arg.history;
        let transfer_history = transfer_history;
        let block_history = block_history;
        loop {
            let incoming_transactions =
                incoming_transactions.lock().expect("Lock failure").to_vec();
            let (next_height, previous_digest) = match block_history
                .lock()
                .expect("Lock failure")
                .sorted_blocks_by_height()
                .last()
            {
                Some(block) => (block.height().next(), block.digest().clone()),
                None => (BlockHeight::genesis(), BlockDigest::digest(&[])),
            };
            let block_src = BlockSource::new(
                next_height,
                incoming_transactions,
                previous_digest,
                DIFFICULTY.clone(),
                rand::thread_rng().gen(),
                &secret_address,
                blockchain_core::block::block_coin_generation_rule,
            );

            if let Ok(block_src) = block_src {
                if let Ok(block) = block_src.try_into_block() {
                    info!("New block may be found. Verifiying...");
                    let res = {
                        let transfer_history = transfer_history.lock().expect("Lock failure");
                        let block_history = block_history.lock().expect("Lock failure");
                        verify_block_after_mining(block, &transfer_history, &block_history)
                    };
                    match res {
                        Ok(block) => {
                            info!("Verified new block.");

                            // Publish block on finding new block
                            let digest = hex::encode(block.digest());
                            match block_publisher.publish(&block).await {
                                Ok(_) => info!("Published the latest block. Digest: {}", digest),
                                Err(e) => error!("Error during publishing a block. {}", e),
                            }

                            let mut transfer_history =
                                transfer_history.lock().expect("Lock failure");
                            let mut block_history = block_history.lock().expect("Lock failure");

                            match transfer_history.push_block(&block) {
                                Ok(_) => info!("Saved new block's transfer."),
                                Err(e) => error!("Error during adding new block's transfer. {}", e),
                            }
                            match block_history.push_block(block) {
                                Ok(_) => info!("Saved new block."),
                                Err(e) => error!("Error during adding new block. {}", e),
                            }

                            info!("Added new block into history.");

                            match save_block_history(&history_path, &block_history) {
                                Ok(_) => info!("Saved new block locally."),
                                Err(e) => error!("Error during saving new block locally. {}", e),
                            }
                        }
                        Err(e) => error!("Error during verifying block. {}", e),
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    });

    info!("Initialization done. A blockchain-fullnode runnning...");

    transaction_subsctiber_join_handle.await?;
    block_subscriber_join_handle.await?;
    block_publisher_join_handle.await?;
    mining_join_handle.await?;

    Ok(())
}
