use crate::block::BlockHeight;
use crate::signature::Signature;
use crate::timestamp::Timestamp;
use crate::verification::Verified;
use crate::VerifiedBlock;
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

type TransactionBranch = crate::transfer::TransactionBranch<Verified>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key(Signature, Timestamp);

#[derive(Debug, Clone)]
pub struct TransferHistory {
    status_map: HashMap<Key, TransferState>,
}

impl TransferHistory {
    pub fn new() -> Self {
        Self {
            status_map: HashMap::new(),
        }
    }

    pub fn state_of(&self, transfer: &TransactionBranch) -> TransferState {
        let key = Key(transfer.sign().clone(), transfer.timestamp());
        self.status_map
            .get(&key)
            .copied()
            .unwrap_or(TransferState::Unlisted)
    }

    pub fn push_block(&mut self, block: &VerifiedBlock) -> Result<(), TransferHistoryError> {
        // A block contains double-spending input?
        if !block.inputs().map(TransactionBranch::sign).all_unique() {
            return Err(TransferHistoryError::DoubleSpending);
        }

        // Scan next status of each transfer
        let inputs = block
            .inputs()
            .map(|t| self.next_state_input(t).map(|s| (t, s)));

        let outputs = block
            .outputs()
            .map(|t| self.next_state_output(t).map(|s| (t, s)));

        let next_states = inputs.chain(outputs).collect::<Result<Vec<_>, _>>()?;

        for (transfer, status) in next_states.into_iter() {
            let key = Key(transfer.sign().clone(), transfer.timestamp());
            self.status_map
                .entry(key)
                .and_modify(|s| *s = status)
                .or_insert(status);
        }

        Ok(())
    }

    pub fn remove_block(&mut self, block: &VerifiedBlock) -> Result<(), TransferHistoryError> {
        // Scan next status of each transfer
        let next_states = block
            .inputs()
            .chain(block.outputs())
            .map(|t| self.previous_state(t).map(|s| (t, s)))
            .collect::<Result<Vec<_>, _>>()?;

        // Update status
        for (transfer, status) in next_states.into_iter() {
            let key = Key(transfer.sign().clone(), transfer.timestamp());
            self.status_map
                .entry(key)
                .and_modify(|s| *s = status)
                .or_insert(status);
        }
        Ok(())
    }

    fn next_state_input(
        &self,
        input: &TransactionBranch,
    ) -> Result<TransferState, TransferHistoryError> {
        match self.state_of(input) {
            TransferState::Unlisted => Err(TransferHistoryError::Unlisted),
            TransferState::Unused => Ok(TransferState::Used),
            TransferState::Used => Err(TransferHistoryError::DoubleSpending),
        }
    }

    fn next_state_output(
        &self,
        output: &TransactionBranch,
    ) -> Result<TransferState, TransferHistoryError> {
        match self.state_of(output) {
            TransferState::Unlisted => Ok(TransferState::Unused),
            TransferState::Unused | TransferState::Used => Err(TransferHistoryError::Collision),
        }
    }

    fn previous_state(
        &self,
        transfer: &TransactionBranch,
    ) -> Result<TransferState, TransferHistoryError> {
        match self.state_of(transfer) {
            TransferState::Unlisted => Err(TransferHistoryError::Unlisted),
            TransferState::Unused => Ok(TransferState::Unused),
            TransferState::Used => Ok(TransferState::Unused),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    Unlisted,
    Unused,
    Used,
}

#[derive(Debug)]
pub enum TransferHistoryError {
    DoubleSpending,
    Collision,
    Unlisted,
}

impl Display for TransferHistoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransferHistoryError::Unlisted => write!(f, "Transfer has not appeared in history"),
            TransferHistoryError::DoubleSpending => write!(f, "Transfer has already been spent"),
            TransferHistoryError::Collision => write!(f, "Transfer sign and timestamp collides"),
        }
    }
}

impl Error for TransferHistoryError {}

#[derive(Debug, Clone)]
pub struct BlockHistory {
    /// Blocks, sorted by its height
    blocks: Vec<VerifiedBlock>,
}

impl BlockHistory {
    pub fn new() -> Self {
        Self { blocks: vec![] }
    }

    pub fn sorted_blocks_by_height(&self) -> &[VerifiedBlock] {
        &self.blocks
    }

    pub fn block_at(&self, height: BlockHeight) -> Option<&VerifiedBlock> {
        self.blocks.iter().find(|b| b.height() == height)
    }

    pub fn push_block(&mut self, block: VerifiedBlock) -> Result<(), BlockHistoryError> {
        match self.blocks.last() {
            Some(prev) if prev.height().next() != block.height() => {
                Err(BlockHistoryError::InvalidHeight)
            }
            Some(prev) if prev.digest() != block.previous_digest() => {
                Err(BlockHistoryError::CorruptDigestChain)
            }
            _ => {
                self.blocks.push(block);
                Ok(())
            }
        }
    }

    pub fn pop_block(&mut self) -> Option<VerifiedBlock> {
        self.blocks.pop()
    }
}

#[derive(Debug)]
pub enum BlockHistoryError {
    InvalidHeight,
    CorruptDigestChain,
}

impl Display for BlockHistoryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlockHistoryError::InvalidHeight => {
                write!(f, "Block's height is not next of the last block")
            }
            BlockHistoryError::CorruptDigestChain => write!(
                f,
                "Block's previous digest does not match with the last block"
            ),
        }
    }
}

impl Error for BlockHistoryError {}
