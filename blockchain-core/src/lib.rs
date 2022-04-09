pub mod account;
pub mod block;
pub mod coin;
pub mod difficulty;
pub mod digest;
pub mod history;
pub mod pace_maker;
pub mod reward;
pub mod signature;
pub mod timestamp;
pub mod transaction;
pub mod transfer;
pub mod verification;

use verification::{Verified, Yet};

pub type UnverifiedBlock = block::Block<Yet, Yet, Yet, Yet, Yet, Yet>;
pub type VerifiedBlock = block::Block<Verified, Verified, Verified, Verified, Verified, Verified>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
