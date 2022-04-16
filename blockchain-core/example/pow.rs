use blockchain_core::account::*;
use blockchain_core::block::*;
use blockchain_core::coin::Coin;
use blockchain_core::difficulty::Difficulty;
use blockchain_core::digest::BlockDigest;
use blockchain_core::transaction::*;
use blockchain_core::transfer::*;

fn main() {
    let input_sender = SecretAddress::create();
    let reliever = SecretAddress::create();
    let output_receiver = SecretAddress::create().to_public_address();
    let miner = SecretAddress::create();

    // Miner will take 1 coin under this situation
    let tx = {
        let input = Transfer::offer(&input_sender, reliever.to_public_address(), Coin::from(10));
        let output = Transfer::offer(&reliever, output_receiver, Coin::from(9));
        Transaction::offer(&reliever, vec![input], vec![output])
            .verify_transaction()
            .unwrap()
    };

    // Block search process
    let height = BlockHeight::genesis();
    let previous_digest = BlockDigest::digest(&[]);
    let difficulty = Difficulty::new(8);
    let nonce = 0;
    let gen_rule = |_: BlockHeight| Coin::from(1);

    let mut block_source = BlockSource::new(
        height,
        vec![tx],
        previous_digest,
        difficulty.clone(),
        nonce,
        &miner,
        gen_rule,
    )
    .unwrap();

    let mut try_count = 0;

    let block = loop {
        *block_source.nonce_mut() = rand::random();
        try_count += 1;

        println!("Try {}", try_count);
        match block_source.try_into_block() {
            Ok(block) => break block,
            Err(source) => block_source = source,
        }
    };

    // Block verification
    let block = block.verify_transaction_relation(gen_rule).unwrap();
    let block = block.verify_utxo(|_| true).unwrap();
    let block = block.verify_digest().unwrap();
    let block = block.verify_previous_block(|_| None, |_| None).unwrap();
    let block = block.verify_difficulty(&difficulty).unwrap();

    // Display block json
    let ser = serde_json::to_string(&block).unwrap();
    println!("{}", ser);

    // Deserialization to verification process
    let de = serde_json::from_str::<Block<_, _, _, _, _, _>>(&ser).unwrap();

    let de = de.verify_transaction_itself().unwrap();
    let de = de.verify_transaction_relation(gen_rule).unwrap();
    let de = de.verify_utxo(|_| true).unwrap();
    let de = de.verify_digest().unwrap();
    let de = de.verify_previous_block(|_| None, |_| None).unwrap();
    let de = de.verify_difficulty(&difficulty).unwrap();

    assert_eq!(de, block);
}
