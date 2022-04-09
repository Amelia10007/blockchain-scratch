use crate::account::{Address, SecretAddress};
use crate::coin::Coin;
use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::verification::{Verified, Yet};
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reward<T> {
    receiver: Address,
    quantity: Coin,
    timestamp: Timestamp,
    sign: Signature,
    _phantom: PhantomData<fn() -> T>,
}

impl<T> Reward<T> {
    pub fn quantity(&self) -> Coin {
        self.quantity
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub fn sign(&self) -> &Signature {
        &self.sign
    }
}

impl Reward<Verified> {
    pub(crate) fn offer(receiver: &SecretAddress, quantity: Coin) -> Self {
        let timestamp = Timestamp::now();

        let sign = {
            let mut builder = SignatureBuilder::new();
            build_signature_source(
                &receiver.to_public_address(),
                quantity,
                timestamp,
                &mut builder,
            );
            receiver.sign(&builder.finalize())
        };

        Self {
            receiver: receiver.to_public_address(),
            quantity,
            timestamp,
            sign,
            _phantom: PhantomData,
        }
    }
}

impl Reward<Yet> {
    pub fn verify(self) -> Result<Reward<Verified>, RewardError> {
        let signature_source = self.build_signature_source();

        if self.receiver.verify(&signature_source, &self.sign) {
            let reward = Reward {
                receiver: self.receiver,
                quantity: self.quantity,
                timestamp: self.timestamp,
                sign: self.sign,
                _phantom: PhantomData,
            };
            Ok(reward)
        } else {
            Err(RewardError(self))
        }
    }
}

impl<'de> Deserialize<'de> for Reward<Yet> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Temporary tipe for deserialization
        #[derive(Deserialize)]
        struct Inner {
            receiver: Address,
            quantity: Coin,
            timestamp: Timestamp,
            sign: Signature,
        }

        let inner = Inner::deserialize(deserializer)?;

        let reward = Reward {
            receiver: inner.receiver,
            quantity: inner.quantity,
            timestamp: inner.timestamp,
            sign: inner.sign,
            _phantom: PhantomData,
        };
        Ok(reward)
    }
}

impl<T> SignatureSource for Reward<T> {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        build_signature_source(&self.receiver, self.quantity, self.timestamp, builder);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct RewardError(Reward<Yet>);

impl Display for RewardError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid sign")
    }
}

impl Error for RewardError {}

fn build_signature_source(
    receiver: &Address,
    quantity: Coin,
    timestamp: Timestamp,
    builder: &mut SignatureBuilder,
) {
    receiver.write_bytes(builder);
    quantity.write_bytes(builder);
    timestamp.write_bytes(builder);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde() {
        let receiver = SecretAddress::create();
        let reward = Reward::offer(&receiver, Coin::from(42));

        let ser = serde_json::to_string(&reward).unwrap();
        let de = serde_json::from_str::<Reward<_>>(&ser).unwrap();

        assert_eq!(Ok(reward), de.verify());
    }

    #[test]
    fn test_serde_corrupt_quantity() {
        let receiver = SecretAddress::create();
        let mut reward = Reward::offer(&receiver, Coin::from(42));
        reward.quantity = Coin::from(1); // Tampering!

        let ser = serde_json::to_string(&reward).unwrap();
        let de = serde_json::from_str::<Reward<_>>(&ser).unwrap();

        assert!(de.verify().is_err());
    }
}
