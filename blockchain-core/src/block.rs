use crate::account::SecretAddress;
use crate::coin::Coin;
use crate::difficulty::Difficulty;
use crate::digest::BlockDigest;
use crate::signature::{SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::transaction::TransactionError;
use crate::transfer::{Generation, TransactionBranch, Transfer};
use crate::verification::{Verified, Yet};
use apply::Apply;
use itertools::Itertools;
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

type Transaction<T> = crate::transaction::Transaction<T, T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlockHeight(u64);

impl BlockHeight {
    pub const fn genesis() -> Self {
        Self(0)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    pub fn previous(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }
}

impl SignatureSource for BlockHeight {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&self.0.to_le_bytes());
    }
}

#[derive(Debug, Clone)]
pub struct BlockSource {
    height: BlockHeight,
    transactions: Vec<Transaction<Verified>>,
    timestamp: Timestamp,
    previous_digest: BlockDigest,
    difficulty: Difficulty,
    nonce: u64,
    digest_source_except_nonce: Vec<u8>,
}

impl BlockSource {
    pub fn new<F>(
        height: BlockHeight,
        transactions: Vec<Transaction<Verified>>,
        previous_digest: BlockDigest,
        difficulty: Difficulty,
        nonce: u64,
        reward_receiver: &SecretAddress,
        mut gen_rule: F,
    ) -> Result<Self, TransactionError>
    where
        F: FnMut(BlockHeight) -> Coin,
    {
        let gen_tx = {
            let in_qty = transactions
                .iter()
                .flat_map(Transaction::inputs)
                .map(TransactionBranch::quantity)
                .sum::<Coin>();
            let o_qty = transactions
                .iter()
                .flat_map(Transaction::outputs)
                .map(TransactionBranch::quantity)
                .sum::<Coin>();
            let r_qty = gen_rule(height) + in_qty - o_qty;

            // Generation transaction
            let inputs: Vec<Transfer<_>> = vec![];
            let outputs = vec![Generation::offer(&reward_receiver, r_qty)];
            crate::transaction::Transaction::offer(reward_receiver, inputs, outputs)
                .verify_transaction()?
        };

        let transactions = transactions
            .into_iter()
            .chain(std::iter::once(gen_tx))
            .sorted_by_key(Transaction::timestamp)
            .collect_vec();

        let timestamp = Timestamp::now();

        let digest_source_except_nonce = builde_digest_source_except_nonce(
            height,
            &transactions,
            &timestamp,
            &previous_digest,
            &difficulty,
        )
        .finalize();

        let source = Self {
            height,
            transactions,
            timestamp,
            previous_digest,
            difficulty,
            nonce,
            digest_source_except_nonce,
        };
        Ok(source)
    }

    pub fn nonce_mut(&mut self) -> &mut u64 {
        &mut self.nonce
    }

    pub fn try_into_block(self) -> Result<Block<Verified, Yet, Yet, Yet, Yet, Yet>, BlockSource> {
        let digest = build_digest_source_from_except_nonce(
            self.digest_source_except_nonce.clone(),
            self.nonce,
        )
        .finalize()
        .apply(|bytes| BlockDigest::digest(&bytes));

        if self.difficulty.verify_digest(&digest) {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                timestamp: self.timestamp,
                previous_digest: self.previous_digest,
                difficulty: self.difficulty,
                nonce: self.nonce,
                digest,
                _phantom: PhantomData,
            };
            Ok(block)
        } else {
            Err(self)
        }
    }
}

/// ## Verification process using Generics:
/// Each generic parameter is `Verified` or `Yet`.
/// - VT: Transaction self check
/// - VTS: Transactions relationship check using generation function
/// - VU: transaction-Utxo judge using utxo history
/// - VP: previous block check by using previous digest and timestamp
/// - VDG: digest matching
/// - VDI: difficulty check using block history and Proof-of-Work
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Block<VT, VTS, VU, VP, VDG, VDI> {
    height: BlockHeight,
    /// All transfers must be UTXO.
    /// Transactions must be sorted by its timestamp.
    transactions: Vec<Transaction<VT>>,
    /// Block creation time, which must be later than any transactions in the block.
    timestamp: Timestamp,
    /// Digest of the previous block.
    previous_digest: BlockDigest,
    /// Difficulty in finding the block.
    difficulty: Difficulty,
    /// PoW key.
    nonce: u64,
    /// Digest of all data of the block except for this block's digest.
    digest: BlockDigest,
    /// Verification process
    #[serde(skip_serializing)]
    _phantom: PhantomData<fn() -> (VTS, VU, VP, VDG, VDI)>,
}

impl<VT, VTS, VU, VP, VDG, VDI> Block<VT, VTS, VU, VP, VDG, VDI> {
    pub fn height(&self) -> BlockHeight {
        self.height
    }

    pub fn transactions(&self) -> &[Transaction<VT>] {
        &self.transactions
    }

    pub fn inputs(&self) -> impl Iterator<Item = &TransactionBranch<VT>> + '_ {
        self.transactions.iter().flat_map(Transaction::inputs)
    }

    pub fn outputs(&self) -> impl Iterator<Item = &TransactionBranch<VT>> + '_ {
        self.transactions.iter().flat_map(Transaction::outputs)
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub fn previous_digest(&self) -> &BlockDigest {
        &self.previous_digest
    }

    pub fn difficulty(&self) -> &Difficulty {
        &self.difficulty
    }

    pub fn digest(&self) -> &BlockDigest {
        &self.digest
    }
}

impl<VTS, VU, VP, VDG, VDI> Block<Yet, VTS, VU, VP, VDG, VDI> {
    pub fn verify_transaction_itself(
        self,
    ) -> Result<Block<Verified, VTS, VU, VP, VDG, VDI>, BlockError> {
        // Verify each tx itself
        let transactions = self
            .transactions
            .into_iter()
            .map(Transaction::verify)
            .collect::<Result<Vec<_>, _>>()
            .map_err(BlockError::Transaction)?;

        let block = Block {
            height: self.height,
            transactions,
            timestamp: self.timestamp,
            previous_digest: self.previous_digest,
            difficulty: self.difficulty,
            nonce: self.nonce,
            digest: self.digest,
            _phantom: PhantomData,
        };

        Ok(block)
    }
}

impl<VT, VU, VP, VDG, VDI> Block<VT, Yet, VU, VP, VDG, VDI> {
    pub fn verify_transaction_relation<F>(
        self,
        mut gen_rule: F,
    ) -> Result<Block<VT, Verified, VU, VP, VDG, VDI>, BlockError>
    where
        F: FnMut(BlockHeight) -> Coin,
    {
        // Timestamp check
        if self
            .transactions
            .iter()
            .map(Transaction::timestamp)
            .any(|stamp| stamp > self.timestamp)
        {
            return Err(BlockError::TransactionTimestamp);
        }
        // Timestamp sorted check
        if !is_sorted::IsSorted::is_sorted(
            &mut self.transactions.iter().map(Transaction::timestamp),
        ) {
            return Err(BlockError::TransactionTimestamp);
        }

        // Quantity check
        let in_qty = self
            .transactions
            .iter()
            .flat_map(Transaction::inputs)
            .map(TransactionBranch::quantity)
            .sum::<Coin>();
        let o_qty = self
            .transactions
            .iter()
            .flat_map(Transaction::outputs)
            .map(TransactionBranch::quantity)
            .sum::<Coin>();
        let r_qty = gen_rule(self.height);

        if in_qty + r_qty != o_qty {
            return Err(BlockError::TransactionQuantity);
        }

        let block = Block {
            height: self.height,
            transactions: self.transactions,
            timestamp: self.timestamp,
            previous_digest: self.previous_digest,
            difficulty: self.difficulty,
            nonce: self.nonce,
            digest: self.digest,
            _phantom: PhantomData,
        };

        Ok(block)
    }
}

impl<VP, VDG, VDI> Block<Verified, Verified, Yet, VP, VDG, VDI> {
    pub fn verify_utxo<F>(
        self,
        mut utxo_judge: F,
    ) -> Result<Block<Verified, Verified, Verified, VP, VDG, VDI>, BlockError>
    where
        F: FnMut(&[Transaction<Verified>]) -> bool,
    {
        let all_utxo = utxo_judge(&self.transactions);

        if all_utxo {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                timestamp: self.timestamp,
                previous_digest: self.previous_digest,
                difficulty: self.difficulty,
                nonce: self.nonce,
                digest: self.digest,
                _phantom: PhantomData,
            };
            Ok(block)
        } else {
            Err(BlockError::Utxo)
        }
    }
}

impl<VT, VTS, VU, VDG, VDI> Block<VT, VTS, VU, Yet, VDG, VDI> {
    pub fn verify_previous_block<'a, F1, F2>(
        self,
        mut digest_history: F1,
        mut timestamp_history: F2,
    ) -> Result<Block<VT, VTS, VU, Verified, VDG, VDI>, BlockError>
    where
        F1: FnMut(BlockHeight) -> Option<&'a BlockDigest>,
        F2: FnMut(BlockHeight) -> Option<Timestamp>,
    {
        match self.height.previous() {
            Some(h) => {
                let digest = digest_history(h);
                let stamp = timestamp_history(h);

                match (digest, stamp) {
                    (Some(digest), Some(stamp))
                        if digest == &self.previous_digest && stamp < self.timestamp =>
                    {
                        let block = Block {
                            height: self.height,
                            transactions: self.transactions,
                            timestamp: self.timestamp,
                            previous_digest: self.previous_digest,
                            difficulty: self.difficulty,
                            nonce: self.nonce,
                            digest: self.digest,
                            _phantom: PhantomData,
                        };
                        Ok(block)
                    }
                    _ => Err(BlockError::Chain),
                }
            }
            // This is genesis block
            None => {
                let block = Block {
                    height: self.height,
                    transactions: self.transactions,
                    timestamp: self.timestamp,
                    previous_digest: self.previous_digest,
                    difficulty: self.difficulty,
                    nonce: self.nonce,
                    digest: self.digest,
                    _phantom: PhantomData,
                };
                Ok(block)
            }
        }
    }
}

impl<VT, VTS, VU, VP, VDI> Block<VT, VTS, VU, VP, Yet, VDI> {
    pub fn verify_digest(self) -> Result<Block<VT, VTS, VU, VP, Verified, VDI>, BlockError> {
        let digest_source = build_digest_source(
            self.height,
            &self.transactions,
            &self.timestamp,
            &self.previous_digest,
            &self.difficulty,
            self.nonce,
        )
        .finalize();
        let digest = BlockDigest::digest(&digest_source);

        if digest == self.digest {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                timestamp: self.timestamp,
                previous_digest: self.previous_digest,
                difficulty: self.difficulty,
                nonce: self.nonce,
                digest: self.digest,
                _phantom: PhantomData,
            };
            Ok(block)
        } else {
            Err(BlockError::Digest)
        }
    }
}

impl<VT, VTS, VU, VP, VDG> Block<VT, VTS, VU, VP, VDG, Yet> {
    pub fn verify_difficulty(
        self,
        expected_difficulty: &Difficulty,
    ) -> Result<Block<VT, VTS, VU, VP, VDG, Verified>, BlockError> {
        if &self.difficulty < expected_difficulty {
            return Err(BlockError::InsufficientDifficulty);
        }

        if expected_difficulty.verify_digest(&self.digest) {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                timestamp: self.timestamp,
                previous_digest: self.previous_digest,
                difficulty: self.difficulty,
                nonce: self.nonce,
                digest: self.digest,
                _phantom: PhantomData,
            };
            Ok(block)
        } else {
            Err(BlockError::PoWFailure)
        }
    }
}

impl<'de> Deserialize<'de> for Block<Yet, Yet, Yet, Yet, Yet, Yet> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Temporary tipe for deserialization
        #[derive(Deserialize)]
        struct Inner {
            height: BlockHeight,
            transactions: Vec<Transaction<Yet>>,
            timestamp: Timestamp,
            previous_digest: BlockDigest,
            difficulty: Difficulty,
            nonce: u64,
            digest: BlockDigest,
        }

        let inner = Inner::deserialize(deserializer)?;

        let block = Block {
            height: inner.height,
            transactions: inner.transactions,
            timestamp: inner.timestamp,
            previous_digest: inner.previous_digest,
            difficulty: inner.difficulty,
            nonce: inner.nonce,
            digest: inner.digest,
            _phantom: PhantomData,
        };
        Ok(block)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BlockError {
    Transaction(TransactionError),
    TransactionQuantity,
    TransactionTimestamp,
    Utxo,
    Chain,
    Digest,
    InsufficientDifficulty,
    PoWFailure,
}

impl Display for BlockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::Transaction(e) => write!(f, "Block contains an invalid transaction: {}", e),
            BlockError::TransactionQuantity => write!(f, "Invalid transactio quantity balance"),
            BlockError::TransactionTimestamp => {
                write!(f, "Block contains a newer transaction than itself")
            }
            BlockError::Utxo => write!(f, "Block contains not-utxo transfer or coin generation"),
            BlockError::Chain => write!(f, "Block is isolated from chain"),
            BlockError::Digest => write!(f, "Digest mismatch"),
            BlockError::InsufficientDifficulty => write!(f, "Insufficient difficulty"),
            BlockError::PoWFailure => write!(f, "Proof-of-Work verification failure"),
        }
    }
}

impl Error for BlockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            BlockError::Transaction(e) => Some(e),
            _ => None,
        }
    }
}

fn builde_digest_source_except_nonce<VT>(
    height: BlockHeight,
    transactions: &[Transaction<VT>],
    timestamp: &Timestamp,
    previous_digest: &BlockDigest,
    difficulty: &Difficulty,
) -> SignatureBuilder {
    let mut builder = SignatureBuilder::new();
    height.write_bytes(&mut builder);
    transactions.write_bytes(&mut builder);
    timestamp.write_bytes(&mut builder);
    previous_digest.write_bytes(&mut builder);
    difficulty.write_bytes(&mut builder);
    builder
}

fn build_digest_source_from_except_nonce(
    digest_source_except_nonce: Vec<u8>,
    nonce: u64,
) -> SignatureBuilder {
    let mut builder = SignatureBuilder::from(digest_source_except_nonce);
    builder.write_bytes(&nonce.to_le_bytes());
    builder
}

fn build_digest_source<VT>(
    height: BlockHeight,
    transactions: &[Transaction<VT>],
    timestamp: &Timestamp,
    previous_digest: &BlockDigest,
    difficulty: &Difficulty,
    nonce: u64,
) -> SignatureBuilder {
    let builder = builde_digest_source_except_nonce(
        height,
        transactions,
        timestamp,
        previous_digest,
        difficulty,
    );
    build_digest_source_from_except_nonce(builder.finalize(), nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn difficulty() -> Difficulty {
        Difficulty::new(1)
    }

    fn generation_rule(_: BlockHeight) -> Coin {
        Coin::from(1)
    }

    fn create_unverified_genesis_block() -> Block<Verified, Yet, Yet, Yet, Yet, Yet> {
        let input_sender = SecretAddress::create();
        let reliever = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let miner = SecretAddress::create();

        // Miner will take 1 coin under this situation
        let tx = {
            let input =
                Transfer::offer(&input_sender, reliever.to_public_address(), Coin::from(10));
            let output = Transfer::offer(&reliever, output_receiver, Coin::from(9));
            crate::transaction::Transaction::offer(&reliever, vec![input], vec![output])
                .verify_transaction()
                .unwrap()
        };

        // Block search process
        let height = BlockHeight::genesis();
        let previous_digest = BlockDigest::digest(&[]);
        let nonce = 0;

        let mut block_source = BlockSource::new(
            height,
            vec![tx],
            previous_digest,
            difficulty(),
            nonce,
            &miner,
            generation_rule,
        )
        .unwrap();

        // Proof of work
        loop {
            *block_source.nonce_mut() = rand::random();

            match block_source.try_into_block() {
                Ok(block) => break block,
                Err(source) => block_source = source,
            }
        }
    }

    #[test]
    fn test_genesis_pow_process() {
        let difficulty = difficulty();
        let block = create_unverified_genesis_block();

        let block = block.verify_transaction_relation(generation_rule).unwrap();
        let block = block.verify_utxo(|_| true).unwrap();
        let block = block.verify_digest().unwrap();
        let block = block.verify_previous_block(|_| None, |_| None).unwrap();
        let block = block.verify_difficulty(&difficulty).unwrap();

        // Deserialization to verification process
        let ser = serde_json::to_string(&block).unwrap();
        let de = serde_json::from_str::<Block<_, _, _, _, _, _>>(&ser).unwrap();

        let de = de.verify_transaction_itself().unwrap();
        let de = de.verify_transaction_relation(generation_rule).unwrap();
        let de = de.verify_utxo(|_| true).unwrap();
        let de = de.verify_digest().unwrap();
        let de = de.verify_previous_block(|_| None, |_| None).unwrap();
        let de = de.verify_difficulty(&difficulty).unwrap();

        assert_eq!(de, block);
    }

    #[test]
    fn test_verify_transaction_relation_too_much_quantity() {
        let block = create_unverified_genesis_block();
        let zero_gen_rule = |_: BlockHeight| Coin::from(0);
        // Block coin generation is too much under zero_gen_rule
        let block = block.verify_transaction_relation(zero_gen_rule);

        assert_eq!(Err(BlockError::TransactionQuantity), block);
    }

    #[test]
    fn test_verify_transaction_relation_too_few_quantity() {
        let block = create_unverified_genesis_block();
        let much_gen_rule = |_: BlockHeight| Coin::from(10000);
        // Block coin generation is too few under much_gen_rule
        let block = block.verify_transaction_relation(much_gen_rule);

        assert_eq!(Err(BlockError::TransactionQuantity), block);
    }

    #[test]
    fn test_verify_utxo_fail() {
        let block = create_unverified_genesis_block();
        let block = block.verify_transaction_relation(generation_rule).unwrap();

        let utxo_judge_always_fail = |_: &[Transaction<_>]| false;
        let block = block.verify_utxo(utxo_judge_always_fail);

        assert_eq!(Err(BlockError::Utxo), block);
    }

    #[test]
    fn test_verify_digest_fail() {
        let block = create_unverified_genesis_block();
        let mut block = block.verify_transaction_relation(generation_rule).unwrap();

        block.height = block.height.next(); // Data tampering!

        let block = block.verify_digest();

        assert_eq!(Err(BlockError::Digest), block);
    }

    #[test]
    fn test_verify_difficulty_fail() {
        let block = create_unverified_genesis_block();
        let block = block.verify_transaction_relation(generation_rule).unwrap();

        let too_difficult = Difficulty::new(255);

        let block = block.verify_difficulty(&too_difficult);

        assert_eq!(Err(BlockError::InsufficientDifficulty), block);
    }
}
