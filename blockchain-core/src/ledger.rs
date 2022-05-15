use crate::block::BlockError;
use crate::digest::BlockDigest;
use crate::transition::Transition;
use crate::verification::Verified;
use crate::{Address, Block, Transaction, VerifiedBlock, Yet};
use apply::Also;
use itertools::Itertools;
use slab_tree::{Ancestors, NodeId, NodeMut, NodeRef, RemoveBehavior, Tree};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::hash::Hash;

/// Wrapper for implementation of Hash.
/// Hasher uses sign of transition.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TransitionWrapper(Transition<Verified>);

impl Hash for TransitionWrapper {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.sign().hash(state)
    }
}

impl From<Transition<Verified>> for TransitionWrapper {
    fn from(t: Transition<Verified>) -> Self {
        Self(t)
    }
}

impl AsRef<Transition<Verified>> for TransitionWrapper {
    fn as_ref(&self) -> &Transition<Verified> {
        &self.0
    }
}

#[derive(Debug)]
struct TransferHistory {
    utxos: Vec<Transition<Verified>>,
}

impl TransferHistory {
    fn new() -> Self {
        Self { utxos: vec![] }
    }

    fn utxos(&self) -> impl Iterator<Item = &Transition<Verified>> + '_ {
        self.utxos.iter()
    }

    fn is_utxo(&self, transition: &Transition<Verified>) -> bool {
        self.utxos.iter().find(|u| u.sign() == transition.sign()).is_some()
    }

    fn push_block(&mut self, block: &VerifiedBlock) -> Result<(), TransferHistoryError> {
        // A block contains double-spending input?
        if !block
            .inputs()
            .map(crate::transition::Transition::sign)
            .all_unique()
        {
            return Err(TransferHistoryError::DoubleSpending);
        }

        let mut next_utxos = self.utxos.clone();

        // Verify transactions in order of timestamp
        for tx in block.transactions() {
            for input in tx.inputs() {
                match next_utxos.iter().find(|u| *u == input) {
                    Some(_) => next_utxos.retain(|u| u != input),
                    None => return Err(TransferHistoryError::Unlisted),
                }
            }

            for output in tx.outputs() {
                match next_utxos.iter().find(|u| *u == output) {
                    Some(_) => return Err(TransferHistoryError::Collision),
                    None => next_utxos.push(output.clone()),
                }
            }
        }

        // Update UTXO history if all transaction verification passed
        self.utxos = next_utxos;

        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
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

    pub fn build_utxos(&self, digest: &BlockDigest, holder: &Address) -> Vec<Transition<Verified>> {
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
            // All transaction inputs must be UTXO
            let cond_in = transactions
                .iter()
                .flat_map(Transaction::inputs)
                .all(|i| transfer_history.is_utxo(i));
            // All transaction outputs must not be UTXO
            let cond_out = transactions
                .iter()
                .flat_map(Transaction::outputs)
                .all(|o| !transfer_history.is_utxo(o));

            cond_in && cond_out
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
                    Err(LedgerError::DuplicatedGenesisBlock)
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

#[derive(Debug, PartialEq, Eq)]
pub enum LedgerError {
    IsolatedBlock,
    DuplicatedBlock,
    DuplicatedGenesisBlock,
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
            LedgerError::DuplicatedGenesisBlock => {
                write!(f, "This ledger already has genesis block")
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
            LedgerError::DuplicatedGenesisBlock => None,
            LedgerError::Transfer(e) => Some(e),
            LedgerError::Block(e) => Some(e),
        }
    }
}
