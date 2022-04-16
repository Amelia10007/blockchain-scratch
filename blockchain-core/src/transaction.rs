use crate::account::{Address, SecretAddress};
use crate::coin::Coin;
use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use crate::timestamp::Timestamp;
use crate::transfer::TransactionBranch;
use crate::transfer::Transfer;
use crate::transfer::TransferError;
use crate::verification::{Verified, Yet};
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;

/// ## Verification process using Generics:
/// Each generic parameter is `Verified` or `Yet`.
/// - VTF: TransFer check.
/// - VTX: Transaction check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transaction<VTF, VTX> {
    contractor: Address,
    /// At least 1 input is required.
    /// All receiver of inputs are contractor.
    inputs: Vec<TransactionBranch<VTF>>,
    /// At least 1 output is required.
    /// All signer of outputs are contractor.
    outputs: Vec<TransactionBranch<VTF>>,
    timestamp: Timestamp,
    /// Contractor's sign
    sign: Signature,
    #[serde(skip_serializing)]
    _phantom: PhantomData<fn() -> VTX>,
}

impl<VTR, VTX> Transaction<VTR, VTX> {
    pub fn inputs(&self) -> &[TransactionBranch<VTR>] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[TransactionBranch<VTR>] {
        &self.outputs
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }
}

impl<VTR> Transaction<VTR, Yet> {
    pub fn offer<T, U>(
        contractor: &SecretAddress,
        inputs: Vec<T>,
        outputs: Vec<U>,
    ) -> Transaction<VTR, Yet>
    where
        T: Into<TransactionBranch<VTR>>,
        U: Into<TransactionBranch<VTR>>,
    {
        let inputs = inputs.into_iter().map(Into::into).collect::<Vec<_>>();
        let outputs = outputs.into_iter().map(Into::into).collect::<Vec<_>>();
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
        // At least 1 output is required
        if self.outputs.is_empty() {
            return Err(TransactionError::EmptyOutput);
        }

        // Input's receiver = contractor
        if !self.inputs.is_empty() && self.inputs.iter().any(|i| i.receiver() != &self.contractor) {
            return Err(TransactionError::SenderMismatch);
        }
        // Transfer output's sender = contractor
        // Note: generations in outputs are not checked.
        if self
            .outputs
            .iter()
            .filter_map(TransactionBranch::try_as_transfer)
            .any(|i| i.sender() != &self.contractor)
        {
            return Err(TransactionError::ReceiverMismatch);
        }

        // Input must be equal or smaller than output except for coin generation
        let input_sum = self
            .inputs
            .iter()
            .map(TransactionBranch::quantity)
            .sum::<Coin>();
        let output_sum_except_gen = self
            .outputs
            .iter()
            .filter_map(TransactionBranch::try_as_transfer)
            .map(Transfer::quantity)
            .sum::<Coin>();
        if input_sum < output_sum_except_gen {
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
            return Err(TransactionError::InvalidSign);
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
        self.verify_branch()
            .and_then(Transaction::verify_transaction)
    }
}

impl<VTX> Transaction<Yet, VTX> {
    pub fn verify_branch(self) -> Result<Transaction<Verified, VTX>, TransactionError> {
        let inputs = self
            .inputs
            .into_iter()
            .map(TransactionBranch::verify)
            .collect::<Result<_, _>>()
            .map_err(TransactionError::Transfer)?;
        let outputs = self
            .outputs
            .into_iter()
            .map(TransactionBranch::verify)
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
            inputs: Vec<TransactionBranch<Yet>>,
            outputs: Vec<TransactionBranch<Yet>>,
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
    EmptyOutput,
    /// Outputs' sender is not contractor.
    SenderMismatch,
    /// Inputs' receiver is not contractor.
    ReceiverMismatch,
    /// Inputs' quantity is larger than outputs'.
    QuantityMismatch,
    /// Any transfers' timestamp is later than transaction's.
    InvalidTimestamp,
    /// Contractor's sign is invalid.
    InvalidSign,
}

impl Display for TransactionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionError::Transfer(e) => {
                write!(f, "Transaction contains an invalid transfer: {}", e)
            }
            TransactionError::EmptyOutput => write!(f, "No output in transaction"),
            TransactionError::SenderMismatch => write!(f, "Output's sender mismatch"),
            TransactionError::ReceiverMismatch => write!(f, "Input's receiver mismatch"),
            TransactionError::QuantityMismatch => write!(f, "Quantity mismatch"),
            TransactionError::InvalidTimestamp => write!(f, "Transaction contains newer transfer"),
            TransactionError::InvalidSign => write!(f, "Contractor's sign is invald"),
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
    inputs: &[TransactionBranch<T>],
    outputs: &[TransactionBranch<T>],
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
    use super::*;
    use crate::transfer::Generation;

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
    fn test_verify_only_gen() {
        let contractor = SecretAddress::create();
        let quantity = Coin::from(42);
        let gen = Generation::offer(&contractor, quantity);

        let inputs = Vec::<Transfer<_>>::new();
        let outputs = vec![gen];

        let tx = Transaction::offer(&contractor, inputs, outputs)
            .verify_transaction()
            .unwrap();

        let json = serde_json::to_string(&tx).unwrap();

        let unverified = serde_json::from_str::<Transaction<_, _>>(&json).unwrap();

        assert_eq!(Ok(tx), unverified.verify());
    }

    #[test]
    fn test_verify_transfer_and_gen() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&contractor, output_receiver, quantity).into();
        let gen = Generation::offer(&contractor, quantity).into();

        let inputs = vec![input];
        let outputs: Vec<TransactionBranch<_>> = vec![output, gen];

        let tx = Transaction::offer(&contractor, inputs, outputs)
            .verify_transaction()
            .unwrap();

        let json = serde_json::to_string(&tx).unwrap();

        let unverified = serde_json::from_str::<Transaction<_, _>>(&json).unwrap();

        assert_eq!(Ok(tx), unverified.verify());
    }

    #[test]
    fn test_quantity_too_much_output() {
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

    #[test]
    fn test_verify_error_empty_output() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();

        let input = Transfer::offer(
            &input_sender,
            contractor.to_public_address(),
            Coin::from(10),
        );
        let outputs: Vec<TransactionBranch<_>> = vec![];
        let tx = Transaction::offer(&contractor, vec![input], outputs).verify_transaction();

        assert_eq!(Err(TransactionError::EmptyOutput), tx);
    }

    #[test]
    fn test_verify_error_sender_mismatch() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let mismatched_contractor = SecretAddress::create(); // !

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&mismatched_contractor, output_receiver, quantity);

        let tx = Transaction::offer(&mismatched_contractor, vec![input], vec![output])
            .verify_transaction();

        assert_eq!(Err(TransactionError::SenderMismatch), tx);
    }

    #[test]
    fn test_verify_error_receiver_mismatch() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let mismatched_contractor = SecretAddress::create(); // !

        let input = Transfer::offer(
            &input_sender,
            mismatched_contractor.to_public_address(),
            quantity,
        );
        let output = Transfer::offer(&contractor, output_receiver, quantity);

        let tx = Transaction::offer(&mismatched_contractor, vec![input], vec![output])
            .verify_transaction();

        assert_eq!(Err(TransactionError::ReceiverMismatch), tx);
    }

    #[test]
    fn test_verify_error_input_timestamp() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&contractor, output_receiver, quantity);

        let mut tx = Transaction::offer(&contractor, vec![input], vec![output]);

        // Input is offered later than transaction creation!
        tx.inputs[0] =
            Transfer::offer(&input_sender, contractor.to_public_address(), quantity).into();

        let tx = tx.verify_transaction();

        assert_eq!(Err(TransactionError::InvalidTimestamp), tx);
    }

    #[test]
    fn test_verify_error_output_timestamp() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&contractor, output_receiver.clone(), quantity);

        let mut tx = Transaction::offer(&contractor, vec![input], vec![output]);

        // Input is offered later than transaction creation!
        tx.outputs[0] = Transfer::offer(&contractor, output_receiver, quantity).into();

        let tx = tx.verify_transaction();

        assert_eq!(Err(TransactionError::InvalidTimestamp), tx);
    }

    #[test]
    fn test_verify_error_invalid_sign() {
        let input_sender = SecretAddress::create();
        let contractor = SecretAddress::create();
        let output_receiver = SecretAddress::create().to_public_address();
        let quantity = Coin::from(42);

        let input = Transfer::offer(&input_sender, contractor.to_public_address(), quantity);
        let output = Transfer::offer(&contractor, output_receiver.clone(), quantity);
        let output_tampered = Transfer::offer(&contractor, output_receiver.clone(), Coin::from(1));

        let mut tx = Transaction::offer(&contractor, vec![input], vec![output]);

        // Tamper!
        tx.outputs[0] = output_tampered.into();

        let tx = tx.verify_transaction();

        assert_eq!(Err(TransactionError::InvalidSign), tx);
    }
}
