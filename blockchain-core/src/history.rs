use crate::block::BlockError;
use crate::digest::BlockDigest;
use crate::transfer::TransactionBranch;
use crate::verification::Verified;
use crate::{Address, Block, VerifiedBlock, Yet};
use apply::Also;
use itertools::Itertools;
use slab_tree::{Ancestors, NodeId, NodeMut, NodeRef, RemoveBehavior, Tree};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::hash::Hash;

#[derive(Debug, PartialEq, Eq)]
struct TransactionBranchWrapper(TransactionBranch<Verified>);

impl Hash for TransactionBranchWrapper {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.sign().hash(state)
    }
}

impl From<TransactionBranch<Verified>> for TransactionBranchWrapper {
    fn from(t: TransactionBranch<Verified>) -> Self {
        Self(t)
    }
}

impl AsRef<TransactionBranch<Verified>> for TransactionBranchWrapper {
    fn as_ref(&self) -> &TransactionBranch<Verified> {
        &self.0
    }
}

#[derive(Debug)]
struct TransferHistory {
    status_map: HashMap<TransactionBranchWrapper, State>,
}

impl TransferHistory {
    fn new() -> Self {
        Self {
            status_map: HashMap::new(),
        }
    }

    fn state_of(&self, transfer: &TransactionBranchWrapper) -> State {
        self.status_map
            .get(transfer)
            .copied()
            .unwrap_or(State::Unlisted)
    }

    fn utxos(&self) -> impl Iterator<Item = &TransactionBranchWrapper> + '_ {
        self.status_map
            .iter()
            .filter(|(_, state)| **state == State::Unused)
            .map(|(key, _)| key)
    }

    fn push_block(&mut self, block: &VerifiedBlock) -> Result<(), TransferHistoryError> {
        // A block contains double-spending input?
        if !block
            .inputs()
            .map(crate::transfer::TransactionBranch::sign)
            .all_unique()
        {
            return Err(TransferHistoryError::DoubleSpending);
        }

        // Scan next status of each transfer
        let inputs = block
            .inputs()
            .map(|t| self.next_state_input(&t.clone().into()).map(|s| (t, s)));

        let outputs = block
            .outputs()
            .map(|t| self.next_state_output(&t.clone().into()).map(|s| (t, s)));

        let next_states = inputs.chain(outputs).collect::<Result<Vec<_>, _>>()?;

        for (transfer, status) in next_states.into_iter() {
            let wrapper = transfer.clone().into();
            self.status_map
                .entry(wrapper)
                .and_modify(|s| *s = status)
                .or_insert(status);
        }

        Ok(())
    }

    fn next_state_input(
        &self,
        input: &TransactionBranchWrapper,
    ) -> Result<State, TransferHistoryError> {
        match self.state_of(input) {
            State::Unlisted => Err(TransferHistoryError::Unlisted),
            State::Unused => Ok(State::Used),
            State::Used => Err(TransferHistoryError::DoubleSpending),
        }
    }

    fn next_state_output(
        &self,
        output: &TransactionBranchWrapper,
    ) -> Result<State, TransferHistoryError> {
        match self.state_of(output) {
            State::Unlisted => Ok(State::Unused),
            State::Unused | State::Used => Err(TransferHistoryError::Collision),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
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

/// Block tree ledger
#[derive(Debug)]
pub struct Ledger {
    block_tree: Tree<VerifiedBlock>,
    digest_map: HashMap<BlockDigest, NodeId>,
}

impl Ledger {
    /// Create empty ledger
    pub fn new() -> Self {
        Self {
            block_tree: Tree::new(),
            digest_map: HashMap::new(),
        }
    }

    pub fn get(&self, digest: &BlockDigest) -> Option<&VerifiedBlock> {
        self.node_by_digest(digest).map(|node| node.data())
    }

    pub fn build_utxos(
        &self,
        digest: &BlockDigest,
        holder: &Address,
    ) -> Vec<TransactionBranch<Verified>> {
        let mut transfer_history = TransferHistory::new();

        let blocks = self
            .upstream_chain_from(digest)
            .collect_vec()
            .also(|blocks| blocks.reverse());
        for block in blocks.into_iter() {
            transfer_history.push_block(block).ok();
        }

        transfer_history
            .utxos()
            .map(AsRef::as_ref)
            .filter(|utxo| utxo.receiver() == holder)
            .cloned()
            .collect()
    }

    pub fn search_latest_block(&self) -> Option<&VerifiedBlock> {
        self.digest_map
            .values()
            .map(|&id| self.block_tree.get(id).expect("Invalid id").data())
            .max_by_key(|block| block.height())
    }

    pub fn upstream_chain_from(&self, digest: &BlockDigest) -> BlockchainUpstream<'_> {
        match self.node_by_digest(digest) {
            Some(node) => BlockchainUpstream::Start(node),
            None => BlockchainUpstream::Empty,
        }
    }

    pub fn search_latest_chain(&self) -> BlockchainUpstream<'_> {
        self.search_latest_block()
            .map(|block| self.upstream_chain_from(block.digest()))
            .unwrap_or(BlockchainUpstream::Empty)
    }

    /// Verify block UTXO and digest chain
    pub fn verify_block(
        &self,
        block: Block<Verified, Verified, Yet, Yet, Verified, Verified>,
    ) -> Result<VerifiedBlock, LedgerError> {
        let previous_block = self.node_by_digest(block.previous_digest());

        // Verify previous block info
        let block = block.verify_previous_block(|height, _| match height.previous() {
            Some(previous_height) => match &previous_block {
                Some(previous_block) => previous_block.data().height() == previous_height,
                None => false,
            },
            // Argument block is genesis block. So previous block must not exist.
            None => previous_block.is_none(),
        })?;

        // Build transfer history fron genesis to previous block
        let transfer_history = {
            let blocks = match previous_block {
                Some(block) => block
                    .ancestors()
                    .map(|node| node.data())
                    .collect_vec()
                    .also(|blocks| blocks.reverse()),
                None => vec![],
            };

            let mut transfer_history = TransferHistory::new();
            for block in blocks.into_iter() {
                if let Err(e) = transfer_history.push_block(&block) {
                    return Err(LedgerError::Transfer(e));
                }
            }
            transfer_history
        };

        // Verify transaction
        let block = block.verify_utxo(|transactions| {
            transactions
                .iter()
                .flat_map(|t| t.inputs().iter().chain(t.outputs().iter()))
                .map(|tx_branch| transfer_history.state_of(&tx_branch.clone().into()))
                .all(|s| matches!(s, State::Unlisted | State::Unused))
        })?;

        Ok(block)
    }

    pub fn entry(&mut self, block: VerifiedBlock) -> Result<(), LedgerError> {
        match block.height().previous() {
            Some(previous_height) => {
                let mut previous_node = self
                    .node_mut_by_digest(block.previous_digest())
                    .ok_or(LedgerError::IsolatedBlock)?;
                // Height constraint
                if previous_node.data().height() != previous_height {
                    return Err(LedgerError::IsolatedBlock);
                }
                // Deny duplication
                if previous_node
                    .as_ref()
                    .children()
                    .any(|child| child.data().digest() == block.digest())
                {
                    return Err(LedgerError::DuplicatedBlock);
                }
                //
                let digest = block.digest().clone();
                let id = previous_node.append(block).node_id();
                self.digest_map.insert(digest, id);
                Ok(())
            }
            // Given block is genesis block
            None => {
                if self.block_tree.root().is_none() {
                    let digest = block.digest().clone();
                    let id = self.block_tree.set_root(block);
                    self.digest_map.insert(digest, id);
                    Ok(())
                } else {
                    Err(LedgerError::IsolatedBlock)
                }
            }
        }
    }

    pub fn remove_branch(&mut self, digest: &BlockDigest) -> Option<VerifiedBlock> {
        self.digest_map
            .get(digest)
            .and_then(|&id| self.block_tree.remove(id, RemoveBehavior::DropChildren))
    }

    fn node_by_digest(&self, digest: &BlockDigest) -> Option<NodeRef<'_, VerifiedBlock>> {
        self.digest_map
            .get(digest)
            .and_then(|&id| self.block_tree.get(id))
    }

    fn node_mut_by_digest(&mut self, digest: &BlockDigest) -> Option<NodeMut<'_, VerifiedBlock>> {
        self.digest_map
            .get(digest)
            .and_then(|&id| self.block_tree.get_mut(id))
    }
}

/// Iterate blocks from latest to genesis.
pub enum BlockchainUpstream<'a> {
    Empty,
    Start(NodeRef<'a, VerifiedBlock>),
    Upstream(Ancestors<'a, VerifiedBlock>),
}

impl<'a> Iterator for BlockchainUpstream<'a> {
    type Item = &'a VerifiedBlock;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            BlockchainUpstream::Start(node) => {
                let data = node.data();
                let ancestors = node.ancestors();
                *self = BlockchainUpstream::Upstream(ancestors);
                Some(data)
            }
            BlockchainUpstream::Upstream(ancestors) => ancestors.next().map(|node| node.data()),
            BlockchainUpstream::Empty => None,
        }
    }
}

#[derive(Debug)]
pub enum LedgerError {
    IsolatedBlock,
    DuplicatedBlock,
    Transfer(TransferHistoryError),
    Block(BlockError),
}

impl From<BlockError> for LedgerError {
    fn from(e: BlockError) -> LedgerError {
        LedgerError::Block(e)
    }
}

impl Display for LedgerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::IsolatedBlock => {
                write!(f, "The block is isolated from any branch of chain")
            }
            LedgerError::DuplicatedBlock => {
                write!(f, "Cannot entry a duplicated block into ledger")
            }
            LedgerError::Transfer(e) => e.fmt(f),
            LedgerError::Block(e) => e.fmt(f),
        }
    }
}

impl Error for LedgerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            LedgerError::IsolatedBlock => None,
            LedgerError::DuplicatedBlock => None,
            LedgerError::Transfer(e) => Some(e),
            LedgerError::Block(e) => Some(e),
        }
    }
}
