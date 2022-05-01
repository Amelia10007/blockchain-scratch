pub mod account;
pub mod block;
pub mod coin;
pub mod difficulty;
pub mod digest;
pub mod ledger;
pub mod signature;
pub mod timestamp;
pub mod transaction;
pub mod transition;
pub mod verification;

pub use account::{Address, SecretAddress};
pub use block::{Block, BlockHeight, BlockSource};
pub use coin::Coin;
pub use difficulty::Difficulty;
pub use transaction::Transaction;
pub use transition::{Generation, Transfer, Transition};
pub use verification::{Verified, Yet};

pub type UnverifiedTransaction = Transaction<Yet, Yet>;
pub type VerifiedTransaction = Transaction<Verified, Verified>;
pub type UnverifiedBlock = block::Block<Yet, Yet, Yet, Yet, Yet, Yet>;
pub type VerifiedBlock = block::Block<Verified, Verified, Verified, Verified, Verified, Verified>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
