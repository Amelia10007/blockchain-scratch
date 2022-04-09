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
            build_signature_source(
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
            Err(TransferError(self))
        }
    }
}

impl Transfer<Verified> {
    pub fn offer(sender: &SecretAddress, receiver: Address, quantity: Coin) -> Transfer<Verified> {
        let timestamp = Timestamp::now();

        let sign = {
            let mut builder = SignatureBuilder::new();
            build_signature_source(
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
        build_signature_source(
            &self.sender,
            &self.receiver,
            self.quantity,
            self.timestamp,
            builder,
        );
    }
}

/// Invalid transfer sign
#[derive(Debug, PartialEq, Eq)]
pub struct TransferError(Transfer<Yet>);

impl Display for TransferError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid transfer sign")
    }
}

impl Error for TransferError {}

fn build_signature_source(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify() {
        let sender = SecretAddress::create();
        let receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let transfer = Transfer::offer(&sender, receiver, quantity);

        let json = serde_json::to_string(&transfer).unwrap();
        let verified = serde_json::from_str::<Transfer<_>>(&json).unwrap().verify();

        assert_eq!(Ok(transfer), verified);
    }

    #[test]
    fn test_sign_verify_corrupt() {
        let sender = SecretAddress::create();
        let receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let mut transfer = Transfer::offer(&sender, receiver, quantity);
        transfer.quantity = Coin::from(1); // Tampering!!!

        let json = serde_json::to_string(&transfer).unwrap();
        let verified = serde_json::from_str::<Transfer<_>>(&json).unwrap().verify();

        assert!(verified.is_err());
    }
}
