use crate::account::{Address, SecretAddress};
use crate::coin::Coin;
use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::transfer::Transfer;
use crate::transfer::TransferError;
use crate::verification::{Verified, Yet};
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferVerified;

/// ## Verification process using Generics:
/// Each generic parameter is `Verified` or `Yet`.
/// - VTF: TransFer check.
/// - VTX: Transaction check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transaction<VTF, VTX> {
    contractor: Address,
    /// At least 1 input is required.
    /// All receiver of inputs are contractor.
    inputs: Vec<Transfer<VTF>>,
    /// At least 1 output is required.
    /// All signer of outputs are contractor.
    outputs: Vec<Transfer<VTF>>,
    timestamp: Timestamp,
    /// Contractor's sign
    sign: Signature,
    #[serde(skip_serializing)]
    _phantom: PhantomData<fn() -> VTX>,
}

impl<VTR, VTX> Transaction<VTR, VTX> {
    pub fn inputs(&self) -> &[Transfer<VTR>] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[Transfer<VTR>] {
        &self.outputs
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }
}

impl<VTR> Transaction<VTR, Yet> {
    pub fn offer(
        contractor: &SecretAddress,
        inputs: Vec<Transfer<VTR>>,
        outputs: Vec<Transfer<VTR>>,
    ) -> Transaction<VTR, Yet> {
        let timestamp = Timestamp::now();

        let sign = {
            let mut builder = SignatureBuilder::new();
            build_signature_source(
                &contractor.to_public_address(),
                &inputs,
                &outputs,
                timestamp,
                &mut builder,
            );
            contractor.sign(&builder.finalize())
        };

        Transaction {
            contractor: contractor.to_public_address(),
            inputs,
            outputs,
            timestamp,
            sign,
            _phantom: PhantomData,
        }
    }

    pub fn verify_transaction(self) -> Result<Transaction<VTR, Verified>, TransactionError> {
        // At least 1 input is required
        if self.inputs.is_empty() {
            return Err(TransactionError::Empty);
        }
        // At least 1 output is required
        if self.outputs.is_empty() {
            return Err(TransactionError::Empty);
        }

        // Input's receiver = contractor
        if self.inputs.iter().any(|i| i.receiver() != &self.contractor) {
            return Err(TransactionError::ReceiverMismatch);
        }
        // Output's sender = contractor
        if self.outputs.iter().any(|i| i.sender() != &self.contractor) {
            return Err(TransactionError::SenderMismatch);
        }

        // Input must be equal or smaller than output
        let input_sum = self.inputs.iter().map(|i| i.quantity()).sum::<Coin>();
        let output_sum = self.outputs.iter().map(|i| i.quantity()).sum::<Coin>();
        if input_sum < output_sum {
            return Err(TransactionError::QuantityMismatch);
        }

        // Timestamp
        if self
            .inputs
            .iter()
            .chain(self.outputs.iter())
            .any(|t| t.timestamp() > self.timestamp)
        {
            return Err(TransactionError::InvalidTimestamp);
        }

        // Sign
        let signature_source = self.build_signature_source();
        if !self.contractor.verify(&signature_source, &self.sign) {
            return Err(TransactionError::SignMismatch);
        }

        let tx = Transaction {
            contractor: self.contractor,
            inputs: self.inputs,
            outputs: self.outputs,
            timestamp: self.timestamp,
            sign: self.sign,
            _phantom: PhantomData,
        };
        Ok(tx)
    }
}

impl Transaction<Yet, Yet> {
    pub fn verify(self) -> Result<Transaction<Verified, Verified>, TransactionError> {
        self.verify_transfers()
            .and_then(Transaction::verify_transaction)
    }
}

impl<VTX> Transaction<Yet, VTX> {
    pub fn verify_transfers(self) -> Result<Transaction<Verified, VTX>, TransactionError> {
        let inputs = self
            .inputs
            .into_iter()
            .map(Transfer::verify)
            .collect::<Result<_, _>>()
            .map_err(TransactionError::Transfer)?;
        let outputs = self
            .outputs
            .into_iter()
            .map(Transfer::verify)
            .collect::<Result<_, _>>()
            .map_err(TransactionError::Transfer)?;

        let tx = Transaction {
            contractor: self.contractor,
            inputs,
            outputs,
            timestamp: self.timestamp,
            sign: self.sign,
            _phantom: PhantomData,
        };
        Ok(tx)
    }
}

impl<VTR, VTX> SignatureSource for Transaction<VTR, VTX> {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        build_signature_source(
            &self.contractor,
            &self.inputs,
            &self.outputs,
            self.timestamp,
            builder,
        );
    }
}

impl<'de> Deserialize<'de> for Transaction<Yet, Yet> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Inner {
            contractor: Address,
            inputs: Vec<Transfer<Yet>>,
            outputs: Vec<Transfer<Yet>>,
            timestamp: Timestamp,
            sign: Signature,
        }

        let inner = Inner::deserialize(deserializer)?;

        let tx = Transaction {
            contractor: inner.contractor,
            inputs: inner.inputs,
            outputs: inner.outputs,
            timestamp: inner.timestamp,
            sign: inner.sign,
            _phantom: PhantomData,
        };

        Ok(tx)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransactionError {
    Transfer(TransferError),
    Empty,
    /// Outputs' sender must be contractor.
    SenderMismatch,
    /// Inputs' receiver must be contractor.
    ReceiverMismatch,
    /// Inputs' quantity must be more than outputs'.
    QuantityMismatch,
    /// All transfers' timestamp must be older than transaction's.
    InvalidTimestamp,
    /// Contractor's sign is invalid.
    SignMismatch,
}

impl Display for TransactionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::Transfer(e) => {
                write!(f, "Transaction contains an invalid transfer: {}", e)
            }
            TransactionError::Empty => write!(f, "No input or output in transaction"),
            TransactionError::SenderMismatch => write!(f, "Sender mismatch"),
            TransactionError::ReceiverMismatch => write!(f, "Receiver mismatch"),
            TransactionError::QuantityMismatch => write!(f, "Quantity mismatch"),
            TransactionError::InvalidTimestamp => write!(f, "Transaction contains newer transfer"),
            TransactionError::SignMismatch => write!(f, "Contractor's sign is invald"),
        }
    }
}

impl Error for TransactionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TransactionError::Transfer(e) => Some(e),
            _ => None,
        }
    }
}

fn build_signature_source<T>(
    contractor: &Address,
    inputs: &[Transfer<T>],
    outputs: &[Transfer<T>],
    timestamp: Timestamp,
    builder: &mut SignatureBuilder,
) {
    contractor.write_bytes(builder);
    inputs.write_bytes(builder);
    outputs.write_bytes(builder);
    timestamp.write_bytes(builder);
}

#[cfg(test)]
mod tests {
    use crate::{
        account::SecretAddress, coin::Coin, transaction::TransactionError, transfer::Transfer,
    };

    use super::Transaction;

    #[test]
    fn test_sign_verify() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&contractor, output_receiver, quantity);

        let tx = Transaction::offer(&contractor, vec![input], vec![output])
            .verify_transaction()
            .unwrap();

        let json = serde_json::to_string(&tx).unwrap();

        let unverified = serde_json::from_str::<Transaction<_, _>>(&json).unwrap();

        assert_eq!(Ok(tx), unverified.verify());
    }

    #[test]
    fn test_quantity_mismatch() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();

        let input = Transfer::offer(
            &input_sender,
            contractor.to_public_address(),
            Coin::from(10),
        );
        let output = Transfer::offer(&contractor, output_receiver, Coin::from(11));

        let tx = Transaction::offer(&contractor, vec![input], vec![output]).verify_transaction();

        assert_eq!(Err(TransactionError::QuantityMismatch), tx);
    }
}
