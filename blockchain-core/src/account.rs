use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use apply::Apply;
use ed25519_dalek::{Keypair, PublicKey, Signer, Verifier};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct SecretAddress {
    keypair: Keypair,
}

impl SecretAddress {
    pub fn create() -> Self {
        let keypair = Keypair::generate(&mut rand::thread_rng());
        SecretAddress { keypair }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.keypair.sign(message).apply(Signature::from)
    }

    pub fn to_public_address(&self) -> Address {
        Address {
            publickey: self.keypair.public,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    publickey: PublicKey,
}

impl Address {
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.publickey.verify(message, signature.as_ref()).is_ok()
    }
}

impl SignatureSource for Address {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(self.publickey.as_bytes().as_slice());
    }
}

#[cfg(test)]
mod tests {
    use super::SecretAddress;

    #[test]
    fn test_sign() {
        let secret_address = SecretAddress::create();
        let message = "The altimate answer=42".as_bytes();

        let sign = secret_address.sign(message);

        let address = secret_address.to_public_address();
        assert!(address.verify(message, &sign));
    }

    #[test]
    fn test_corrupt_message() {
        let secret_address = SecretAddress::create();
        let message = "The altimate answer=42".as_bytes();

        let sign = secret_address.sign(message);

        let address = secret_address.to_public_address();
        assert!(!address.verify("The altimate answer=43".as_bytes(), &sign));
    }

    #[test]
    fn test_corrupt_sign() {
        let secret_address = SecretAddress::create();
        let message = "The altimate answer=42".as_bytes();

        let sign = secret_address.sign("The altimate answer=43".as_bytes());

        let address = secret_address.to_public_address();
        assert!(!address.verify(message, &sign));
    }
}
