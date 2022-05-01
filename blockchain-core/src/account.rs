use crate::signature::{Signature, SignatureBuilder, SignatureSource};
use apply::Apply;
use ed25519_dalek::{Keypair, PublicKey, Signer, Verifier};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize)]
pub struct SecretAddress {
    keypair: Keypair,
}

impl SecretAddress {
    pub fn create() -> Self {
        let keypair = Keypair::generate(&mut rand::rngs::OsRng {});
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

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = hex::encode(&self.publickey.as_bytes());
        s.fmt(f)
    }
}

impl FromStr for Address {
    type Err = AddressError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;
        let publickey = PublicKey::from_bytes(&bytes)?;
        let address = Self { publickey };
        Ok(address)
    }
}

#[derive(Debug)]
pub enum AddressError {
    HexDecode(hex::FromHexError),
    Ed25519(ed25519_dalek::ed25519::Error),
}

impl From<hex::FromHexError> for AddressError {
    fn from(e: hex::FromHexError) -> Self {
        AddressError::HexDecode(e)
    }
}

impl From<ed25519_dalek::ed25519::Error> for AddressError {
    fn from(e: ed25519_dalek::ed25519::Error) -> Self {
        AddressError::Ed25519(e)
    }
}

impl Display for AddressError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AddressError::HexDecode(e) => e.fmt(f),
            AddressError::Ed25519(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for AddressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AddressError::HexDecode(e) => Some(e),
            AddressError::Ed25519(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Address, SecretAddress};
    use std::str::FromStr;

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

    #[test]
    fn test_from_str() {
        let address = SecretAddress::create().to_public_address();

        let s = address.to_string();
        let from_str = Address::from_str(&s).unwrap();

        assert_eq!(address, from_str);
    }
}
