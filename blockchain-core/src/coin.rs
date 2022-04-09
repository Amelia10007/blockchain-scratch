use crate::signature::{SignatureBuilder, SignatureSource};
use serde::{Deserialize, Serialize};
use std::iter::Sum;
use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Coin(u64);

impl Coin {
    pub const fn from(quantity: u64) -> Self {
        Self(quantity)
    }
}

impl SignatureSource for Coin {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&self.0.to_le_bytes());
    }
}

impl Default for Coin {
    fn default() -> Self {
        Coin::from(u64::default())
    }
}

impl Add for Coin {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Coin(self.0 + rhs.0)
    }
}

impl Sub for Coin {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Coin(self.0 - rhs.0)
    }
}

impl Sum<Coin> for Coin {
    fn sum<I>(iter: I) -> Coin
    where
        I: Iterator<Item = Coin>,
    {
        iter.fold(Coin::default(), |acc, cur| Coin::from(acc.0 + cur.0))
    }
}

#[test]
fn test_sum() {
    let sum = (1..).take(10).map(Coin::from).sum::<Coin>();

    assert_eq!(Coin(55), sum);
}
