use crate::account::Address;
use crate::account::SecretAddress;
use crate::coin::Coin;
use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::verification::{Verified, Yet};
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transfer<T> {
    sender: Address,
    receiver: Address,
    quantity: Coin,
    timestamp: Timestamp,
    sign: Signature,
    #[serde(skip_serializing)]
    _phantom: PhantomData<fn(T)>,
}

impl<T> Transfer<T> {
    pub fn sender(&self) -> &Address {
        &self.sender
    }

    pub fn receiver(&self) -> &Address {
        &self.receiver
    }

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

impl Transfer<Yet> {
    pub fn verify(self) -> Result<Transfer<Verified>, TransferError> {
        let signature_source = {
            let mut builder = SignatureBuilder::new();
            build_transfer_signature_source(
                &self.sender,
                &self.receiver,
                self.quantity,
                self.timestamp,
                &mut builder,
            );
            builder.finalize()
        };

        if self.sender.verify(&signature_source, &self.sign) {
            Ok(Transfer {
                sender: self.sender,
                receiver: self.receiver,
                quantity: self.quantity,
                timestamp: self.timestamp,
                sign: self.sign,
                _phantom: PhantomData,
            })
        } else {
            Err(TransferError)
        }
    }
}

impl Transfer<Verified> {
    pub fn offer(sender: &SecretAddress, receiver: Address, quantity: Coin) -> Transfer<Verified> {
        let timestamp = Timestamp::now();

        let sign = {
            let mut builder = SignatureBuilder::new();
            build_transfer_signature_source(
                &sender.to_public_address(),
                &receiver,
                quantity,
                timestamp,
                &mut builder,
            );
            let signature_source = builder.finalize();
            sender.sign(&signature_source)
        };

        Transfer {
            sender: sender.to_public_address(),
            receiver,
            quantity,
            timestamp,
            sign,
            _phantom: PhantomData,
        }
    }
}

impl<'de> Deserialize<'de> for Transfer<Yet> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Temporary tipe for deserialization
        #[derive(Deserialize)]
        struct Inner {
            sender: Address,
            receiver: Address,
            quantity: Coin,
            timestamp: Timestamp,
            sign: Signature,
        }

        let inner = Inner::deserialize(deserializer)?;

        let transfer = Transfer {
            sender: inner.sender,
            receiver: inner.receiver,
            quantity: inner.quantity,
            timestamp: inner.timestamp,
            sign: inner.sign,
            _phantom: PhantomData,
        };
        Ok(transfer)
    }
}

impl<T> SignatureSource for Transfer<T> {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        build_transfer_signature_source(
            &self.sender,
            &self.receiver,
            self.quantity,
            self.timestamp,
            builder,
        );
    }
}

/// New issue
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Generation<T> {
    receiver: Address,
    quantity: Coin,
    timestamp: Timestamp,
    sign: Signature,
    #[serde(skip_serializing)]
    _phantom: PhantomData<fn(T)>,
}

impl<T> Generation<T> {
    pub fn receiver(&self) -> &Address {
        &self.receiver
    }

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

impl Generation<Yet> {
    pub fn verify(self) -> Result<Generation<Verified>, TransferError> {
        let signature_source = {
            let mut builder = SignatureBuilder::new();
            build_generation_signature_source(
                &self.receiver,
                self.quantity,
                self.timestamp,
                &mut builder,
            );
            builder.finalize()
        };

        if self.receiver.verify(&signature_source, &self.sign) {
            Ok(Generation {
                receiver: self.receiver,
                quantity: self.quantity,
                timestamp: self.timestamp,
                sign: self.sign,
                _phantom: PhantomData,
            })
        } else {
            Err(TransferError)
        }
    }
}

impl Generation<Verified> {
    pub fn offer(receiver: &SecretAddress, quantity: Coin) -> Generation<Verified> {
        let timestamp = Timestamp::now();

        let sign = {
            let mut builder = SignatureBuilder::new();
            build_generation_signature_source(
                &receiver.to_public_address(),
                quantity,
                timestamp,
                &mut builder,
            );
            let signature_source = builder.finalize();
            receiver.sign(&signature_source)
        };

        Generation {
            receiver: receiver.to_public_address(),
            quantity,
            timestamp,
            sign,
            _phantom: PhantomData,
        }
    }
}

impl<'de> Deserialize<'de> for Generation<Yet> {
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

        let gen = Generation {
            receiver: inner.receiver,
            quantity: inner.quantity,
            timestamp: inner.timestamp,
            sign: inner.sign,
            _phantom: PhantomData,
        };
        Ok(gen)
    }
}

impl<T> SignatureSource for Generation<T> {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        build_generation_signature_source(&self.receiver, self.quantity, self.timestamp, builder);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TransactionBranch<T> {
    Transfer(Transfer<T>),
    Generation(Generation<T>),
}

impl<T> TransactionBranch<T> {
    pub fn receiver(&self) -> &Address {
        match self {
            TransactionBranch::Transfer(t) => t.receiver(),
            TransactionBranch::Generation(g) => g.receiver(),
        }
    }

    pub fn quantity(&self) -> Coin {
        match self {
            TransactionBranch::Transfer(t) => t.quantity(),
            TransactionBranch::Generation(g) => g.quantity(),
        }
    }

    pub fn timestamp(&self) -> Timestamp {
        match self {
            TransactionBranch::Transfer(t) => t.timestamp(),
            TransactionBranch::Generation(g) => g.timestamp(),
        }
    }

    pub fn sign(&self) -> &Signature {
        match self {
            TransactionBranch::Transfer(t) => t.sign(),
            TransactionBranch::Generation(g) => g.sign(),
        }
    }

    pub fn try_as_transfer(&self) -> Option<&Transfer<T>> {
        match self {
            TransactionBranch::Transfer(t) => Some(t),
            TransactionBranch::Generation(_) => None,
        }
    }
}

impl TransactionBranch<Yet> {
    pub fn verify(self) -> Result<TransactionBranch<Verified>, TransferError> {
        match self {
            TransactionBranch::Transfer(t) => t.verify().map(Into::into),
            TransactionBranch::Generation(g) => g.verify().map(Into::into),
        }
    }
}

impl<T> From<Transfer<T>> for TransactionBranch<T> {
    fn from(t: Transfer<T>) -> Self {
        TransactionBranch::Transfer(t)
    }
}

impl<T> From<Generation<T>> for TransactionBranch<T> {
    fn from(g: Generation<T>) -> Self {
        TransactionBranch::Generation(g)
    }
}

impl<'de> Deserialize<'de> for TransactionBranch<Yet> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Temporary tipe for deserialization
        #[derive(Deserialize)]
        pub enum Inner {
            Transfer(Transfer<Yet>),
            Generation(Generation<Yet>),
        }

        let inner = Inner::deserialize(deserializer)?;

        let branch = match inner {
            Inner::Transfer(t) => TransactionBranch::Transfer(t),
            Inner::Generation(g) => TransactionBranch::Generation(g),
        };
        Ok(branch)
    }
}

impl<T> SignatureSource for TransactionBranch<T> {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        match self {
            TransactionBranch::Transfer(t) => t.write_bytes(builder),
            TransactionBranch::Generation(g) => g.write_bytes(builder),
        }
    }
}

/// Invalid transfer sign
#[derive(Debug, PartialEq, Eq)]
pub struct TransferError;

impl Display for TransferError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid transfer sign")
    }
}

impl Error for TransferError {}

fn build_transfer_signature_source(
    sender: &Address,
    receiver: &Address,
    quantity: Coin,
    timestamp: Timestamp,
    builder: &mut SignatureBuilder,
) {
    sender.write_bytes(builder);
    receiver.write_bytes(builder);
    quantity.write_bytes(builder);
    timestamp.write_bytes(builder);
}

fn build_generation_signature_source(
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
    fn test_transfer_sign_verify() {
        let sender = SecretAddress::create();
        let receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let transfer = Transfer::offer(&sender, receiver, quantity);

        let json = serde_json::to_string(&transfer).unwrap();
        let verified = serde_json::from_str::<Transfer<_>>(&json).unwrap().verify();

        assert_eq!(Ok(transfer), verified);
    }

    #[test]
    fn test_transfer_sign_verify_corrupt() {
        let sender = SecretAddress::create();
        let receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let mut transfer = Transfer::offer(&sender, receiver, quantity);
        transfer.quantity = Coin::from(1); // Tampering!!!

        let json = serde_json::to_string(&transfer).unwrap();
        let verified = serde_json::from_str::<Transfer<_>>(&json).unwrap().verify();

        assert!(verified.is_err());
    }

    #[test]
    fn test_generation_sign_verify() {
        let receiver = SecretAddress::create();
        let quantity = Coin::from(42);

        let gen = Generation::offer(&receiver, quantity);

        let json = serde_json::to_string(&gen).unwrap();
        let verified = serde_json::from_str::<Generation<_>>(&json)
            .unwrap()
            .verify();

        assert_eq!(Ok(gen), verified);
    }

    #[test]
    fn test_generation_sign_verify_corrupt() {
        let receiver = SecretAddress::create();
        let quantity = Coin::from(42);

        let mut gen = Generation::offer(&receiver, quantity);
        gen.quantity = Coin::from(1); // Tampering!!!

        let json = serde_json::to_string(&gen).unwrap();
        let verified = serde_json::from_str::<Generation<_>>(&json)
            .unwrap()
            .verify();

        assert!(verified.is_err());
    }

    #[test]
    fn test_branch_transfer_serde() {
        let transfer = {
            let sender = SecretAddress::create();
            let receiver = SecretAddress::create().to_public_address();
            let quantity = Coin::from(42);

            Transfer::offer(&sender, receiver, quantity)
        };

        let branch = TransactionBranch::from(transfer.clone());

        let ser = serde_json::to_string(&branch).unwrap();
        let de = serde_json::from_str::<TransactionBranch<_>>(&ser)
            .unwrap()
            .verify();

        assert_eq!(Ok(TransactionBranch::Transfer(transfer)), de);
    }

    #[test]
    fn test_branch_generation_serde() {
        let gen = {
            let receiver = SecretAddress::create();
            let quantity = Coin::from(42);

            Generation::offer(&receiver, quantity)
        };

        let branch = TransactionBranch::from(gen.clone());

        let ser = serde_json::to_string(&branch).unwrap();
        let de = serde_json::from_str::<TransactionBranch<_>>(&ser)
            .unwrap()
            .verify();

        assert_eq!(Ok(TransactionBranch::Generation(gen)), de);
    }

    #[test]
    fn test_branch_corrupt() {
        let mut gen = {
            let receiver = SecretAddress::create();
            let quantity = Coin::from(42);

            Generation::offer(&receiver, quantity)
        };
        gen.quantity = Coin::from(1); // Tampering!

        let branch = TransactionBranch::from(gen);

        let ser = serde_json::to_string(&branch).unwrap();
        let de = serde_json::from_str::<TransactionBranch<_>>(&ser)
            .unwrap()
            .verify();

        assert_eq!(Err(TransferError), de);
    }
}
