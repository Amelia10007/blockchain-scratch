use crate::block::ExtendedTransfer;
use crate::signature::Signature;
use crate::timestamp::Timestamp;
use crate::VerifiedBlock;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key(Signature, Timestamp);

#[derive(Debug, Clone)]
pub struct TransferHistory {
    status_map: HashMap<Key, TransferStatus>,
}

impl TransferHistory {
    pub fn new() -> Self {
        Self {
            status_map: HashMap::new(),
        }
    }

    pub fn status_of(&self, transfer: &ExtendedTransfer) -> TransferStatus {
        let key = Key(transfer.sign().clone(), transfer.timestamp());
        self.status_map
            .get(&key)
            .copied()
            .unwrap_or(TransferStatus::Unlisted)
    }

    pub fn push_block(&mut self, block: &VerifiedBlock) -> Result<(), TransferHistoryError> {
        // Scan next status of each transfer
        let mut next_map = vec![];
        for input in block.inputs() {
            let status = self.status_of(&ExtendedTransfer::Transfer(input));
            let next = match status {
                TransferStatus::Unlisted => Err(TransferHistoryError::UseUnlistedInput),
                TransferStatus::Unused => Ok(TransferStatus::Used),
                TransferStatus::Used => Err(TransferHistoryError::DoubleSpending),
            }?;
            let key = Key(input.sign().clone(), input.timestamp());
            next_map.push((key, next));
        }

        for output in block.iter_extended_outputs() {
            let status = self.status_of(&output);
            let next = match status {
                TransferStatus::Unlisted => Ok(TransferStatus::Unused),
                TransferStatus::Unused | TransferStatus::Used => {
                    Err(TransferHistoryError::Collision)
                }
            }?;
            let key = Key(output.sign().clone(), output.timestamp());
            next_map.push((key, next));
        }

        // Update status
        for (key, status) in next_map.into_iter() {
            self.status_map
                .entry(key)
                .and_modify(|s| *s = status)
                .or_insert(status);
        }

        Ok(())
    }

    pub fn remove_block(&mut self, block: &VerifiedBlock) {
        unimplemented!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Unlisted,
    Unused,
    Used,
}

pub enum TransferHistoryError {
    UseUnlistedInput,
    DoubleSpending,
    Collision,
}
