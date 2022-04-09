use crate::account::SecretAddress;
use crate::coin::Coin;
use crate::difficulty::Difficulty;
use crate::digest::BlockDigest;
use crate::reward::{Reward, RewardError};
use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::transaction::TransactionError;
use crate::transfer::Transfer;
use crate::verification::{Verified, Yet};
use apply::Apply;
use itertools::Iterate;
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
pub struct BlockSource<VT> {
    height: BlockHeight,
    transactions: Vec<Transaction<VT>>,
    reward: Reward<Verified>,
    timestamp: Timestamp,
    previous_digest: BlockDigest,
    difficulty: Difficulty,
    nonce: u64,
    digest_source_except_nonce: Vec<u8>,
}

impl<VT> BlockSource<VT> {
    pub fn new<F>(
        height: BlockHeight,
        transactions: Vec<Transaction<VT>>,
        previous_digest: BlockDigest,
        difficulty: Difficulty,
        nonce: u64,
        reward_receiver: &SecretAddress,
        mut reward_rule: F,
    ) -> Self
    where
        F: FnMut(BlockHeight) -> Coin,
    {
        let reward = {
            let in_qty = transactions
                .iter()
                .flat_map(|tx| tx.inputs())
                .map(|transfer| transfer.quantity())
                .sum::<Coin>();
            let o_qty = transactions
                .iter()
                .flat_map(|tx| tx.outputs())
                .map(|transfer| transfer.quantity())
                .sum::<Coin>();
            let r_qty = reward_rule(height) + in_qty - o_qty;
            Reward::offer(&reward_receiver, r_qty)
        };

        let timestamp = Timestamp::now();

        let digest_source_except_nonce = builde_digest_source_except_nonce(
            height,
            &transactions,
            &reward,
            &timestamp,
            &previous_digest,
            &difficulty,
        )
        .finalize();

        Self {
            height,
            transactions,
            reward,
            timestamp,
            previous_digest,
            difficulty,
            nonce,
            digest_source_except_nonce,
        }
    }

    pub fn nonce_mut(&mut self) -> &mut u64 {
        &mut self.nonce
    }

    pub fn try_into_block(
        self,
    ) -> Result<Block<VT, Verified, Yet, Yet, Verified, Yet>, BlockSource<VT>> {
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
                reward: self.reward,
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

#[derive(Debug)]
pub enum ExtendedTransfer<'a> {
    Transfer(&'a Transfer<Verified>),
    Reward(&'a Reward<Verified>),
}

impl<'a> ExtendedTransfer<'a> {
    pub fn timestamp(&self) -> Timestamp {
        match self {
            ExtendedTransfer::Transfer(t) => t.timestamp(),
            ExtendedTransfer::Reward(r) => r.timestamp(),
        }
    }

    pub fn sign(&self) -> &'a Signature {
        match self {
            ExtendedTransfer::Transfer(t) => t.sign(),
            ExtendedTransfer::Reward(r) => r.sign(),
        }
    }
}

/// ## Verification process using Generics:
/// Each generic parameter is `Verified` or `Yet`.
/// - VT: Transaction check using transaction-self check, rewards, and timestamp
/// - R: Reward check
/// - VU: transaction-Utxo judge using utxo history
/// - VP: previous block check by using previous digest and timestamp
/// - VDG: digest matching
/// - VDI: difficulty check using block history and Proof-of-Work
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Block<VT, R, VU, VP, VDG, VDI> {
    height: BlockHeight,
    /// All transfers must be UTXO.
    transactions: Vec<Transaction<VT>>,
    /// A special transaction representing transaction fee + new issue.
    reward: Reward<R>,
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
    _phantom: PhantomData<fn() -> (VU, VP, VDG, VDI)>,
}

impl<VT, R, VU, VP, VDG, VDI> Block<VT, R, VU, VP, VDG, VDI> {
    pub fn inputs(&self) -> impl Iterator<Item = &Transfer<VT>> + '_ {
        self.transactions.iter().flat_map(|tx| tx.inputs())
    }

    pub fn outputs(&self) -> impl Iterator<Item = &Transfer<VT>> + '_ {
        self.transactions.iter().flat_map(|tx| tx.outputs())
    }
}

impl<VU, VP, VDG, VDI> Block<Verified, Verified, VU, VP, VDG, VDI> {
    pub fn iter_extended_transfers(&self) -> impl Iterator<Item = ExtendedTransfer<'_>> {
        let inputs = self
            .transactions
            .iter()
            .flat_map(|tx| tx.inputs())
            .map(ExtendedTransfer::Transfer);

        inputs.chain(self.iter_extended_outputs())
    }

    pub fn iter_extended_outputs(&self) -> impl Iterator<Item = ExtendedTransfer<'_>> {
        let outputs = self
            .transactions
            .iter()
            .flat_map(|tx| tx.outputs())
            .map(ExtendedTransfer::Transfer);
        let reward = ExtendedTransfer::Reward(&self.reward);

        outputs.chain(std::iter::once(reward))
    }
}

impl<R, VU, VP, VDG, VDI> Block<Yet, R, VU, VP, VDG, VDI> {
    pub fn verify_transactions(self) -> Result<Block<Verified, R, VU, VP, VDG, VDI>, BlockError> {
        // Verify each tx itself
        let transactions = self
            .transactions
            .into_iter()
            .map(Transaction::verify)
            .collect::<Result<Vec<_>, _>>()
            .map_err(BlockError::Transaction)?;

        // Timestamp check
        if transactions
            .iter()
            .map(|tx| tx.timestamp())
            .any(|stamp| stamp > self.timestamp)
        {
            return Err(BlockError::TransactionTimestamp);
        }

        // Quantity check
        let in_qty = transactions
            .iter()
            .flat_map(|tx| tx.inputs())
            .map(|transfer| transfer.quantity())
            .sum::<Coin>();
        let o_qty = transactions
            .iter()
            .flat_map(|tx| tx.outputs())
            .map(|transfer| transfer.quantity())
            .sum::<Coin>();
        let r_qty = self.reward.quantity();

        if in_qty < o_qty {
            return Err(BlockError::TransactionQuantity);
        }
        if in_qty > o_qty + r_qty {
            return Err(BlockError::RewardQuantity);
        }

        let block = Block {
            height: self.height,
            transactions,
            reward: self.reward,
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
    pub fn verify_rewards<F>(
        self,
        mut reward_rule: F,
    ) -> Result<Block<VT, Verified, VU, VP, VDG, VDI>, BlockError>
    where
        F: FnMut(BlockHeight) -> Coin,
    {
        let reward = self.reward.verify().map_err(BlockError::Reward)?;

        // Quantity check
        let in_qty = self
            .transactions
            .iter()
            .flat_map(|tx| tx.inputs())
            .map(|transfer| transfer.quantity())
            .sum::<Coin>();
        let o_qty = self
            .transactions
            .iter()
            .flat_map(|tx| tx.outputs())
            .map(|transfer| transfer.quantity())
            .sum::<Coin>();

        if reward.quantity() != reward_rule(self.height) + in_qty - o_qty {
            return Err(BlockError::RewardQuantity);
        }

        if reward.timestamp() > self.timestamp {
            return Err(BlockError::RewardTimestamp);
        }

        let block = Block {
            height: self.height,
            transactions: self.transactions,
            reward,
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
        F: FnMut(&ExtendedTransfer<'_>) -> bool,
    {
        let all_utxo = self.iter_extended_transfers().all(|t| utxo_judge(&t));

        if all_utxo {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                reward: self.reward,
                timestamp: self.timestamp,
                previous_digest: self.previous_digest,
                difficulty: self.difficulty,
                nonce: self.nonce,
                digest: self.digest,
                _phantom: PhantomData,
            };
            Ok(block)
        } else {
            Err(BlockError::UsedTransactionOutput)
        }
    }
}

impl<VT, R, VU, VDG, VDI> Block<VT, R, VU, Yet, VDG, VDI> {
    pub fn verify_previous_block<'a, F1, F2>(
        self,
        mut digest_history: F1,
        mut timestamp_history: F2,
    ) -> Result<Block<VT, R, VU, Verified, VDG, VDI>, BlockError>
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
                            reward: self.reward,
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
                    reward: self.reward,
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

impl<VT, R, VU, VP, VDI> Block<VT, R, VU, VP, Yet, VDI> {
    pub fn verify_digest(self) -> Result<Block<VT, R, VU, VP, Verified, VDI>, BlockError> {
        let digest_source = build_digest_source(
            self.height,
            &self.transactions,
            &self.reward,
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
                reward: self.reward,
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

impl<VT, R, VU, VP, VDG> Block<VT, R, VU, VP, VDG, Yet> {
    pub fn verify_difficulty(
        self,
        expected_difficulty: &Difficulty,
    ) -> Result<Block<VT, R, VU, VP, VDG, Verified>, BlockError> {
        if &self.difficulty < expected_difficulty {
            return Err(BlockError::InsufficientDifficulty);
        }

        if expected_difficulty.verify_digest(&self.digest) {
            let block = Block {
                height: self.height,
                transactions: self.transactions,
                reward: self.reward,
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
            reward: Reward<Yet>,
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
            reward: inner.reward,
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
    Reward(RewardError),
    RewardQuantity,
    RewardTimestamp,
    UsedTransactionOutput,
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
            BlockError::Reward(e) => {
                write!(f, "Block contains an invalid reward transaction: {}", e)
            }
            BlockError::RewardQuantity => write!(f, "Invalid reward quantity"),
            BlockError::RewardTimestamp => write!(f, "Invalid reward timestamp"),
            BlockError::UsedTransactionOutput => write!(f, "Block contains a spent transaction"),
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
            BlockError::Reward(e) => Some(e),
            _ => None,
        }
    }
}

fn builde_digest_source_except_nonce<VT, R>(
    height: BlockHeight,
    transactions: &[Transaction<VT>],
    reward: &Reward<R>,
    timestamp: &Timestamp,
    previous_digest: &BlockDigest,
    difficulty: &Difficulty,
) -> SignatureBuilder {
    let mut builder = SignatureBuilder::new();
    height.write_bytes(&mut builder);
    transactions.write_bytes(&mut builder);
    reward.write_bytes(&mut builder);
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

fn build_digest_source<VT, R>(
    height: BlockHeight,
    transactions: &[Transaction<VT>],
    reward: &Reward<R>,
    timestamp: &Timestamp,
    previous_digest: &BlockDigest,
    difficulty: &Difficulty,
    nonce: u64,
) -> SignatureBuilder {
    let builder = builde_digest_source_except_nonce(
        height,
        transactions,
        reward,
        timestamp,
        previous_digest,
        difficulty,
    );
    build_digest_source_from_except_nonce(builder.finalize(), nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_pow_process() {
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
        let difficulty = Difficulty::new(8);
        let nonce = 0;
        let reward_rule = |_: BlockHeight| Coin::from(1);

        let mut block_source = BlockSource::new(
            height,
            vec![tx],
            previous_digest,
            difficulty.clone(),
            nonce,
            &miner,
            reward_rule,
        );

        let block = loop {
            *block_source.nonce_mut() = rand::random();

            match block_source.try_into_block() {
                Ok(block) => break block,
                Err(source) => block_source = source,
            }
        };

        let block = block.verify_utxo(|_| true).unwrap();
        let block = block.verify_previous_block(|_| None, |_| None).unwrap();
        let block = block.verify_difficulty(&difficulty).unwrap();

        assert_eq!(block.reward.quantity(), Coin::from(2));

        // Deserialization to verification process
        let ser = serde_json::to_string(&block).unwrap();
        let de = serde_json::from_str::<Block<_, _, _, _, _, _>>(&ser).unwrap();

        let de = de.verify_transactions().unwrap();
        let de = de.verify_rewards(reward_rule).unwrap();
        let de = de.verify_digest().unwrap();
        let de = de.verify_utxo(|_| true).unwrap();
        let de = de.verify_previous_block(|_| None, |_| None).unwrap();
        let de = de.verify_difficulty(&difficulty).unwrap();

        assert_eq!(de, block);
    }
}
