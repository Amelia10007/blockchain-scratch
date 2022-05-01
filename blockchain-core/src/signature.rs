use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(ed25519_dalek::Signature);

impl Hash for Signature {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.0.as_ref().hash(state);
    }
}

impl From<ed25519_dalek::Signature> for Signature {
    fn from(s: ed25519_dalek::Signature) -> Self {
        Self(s)
    }
}

impl AsRef<ed25519_dalek::Signature> for Signature {
    fn as_ref(&self) -> &ed25519_dalek::Signature {
        &self.0
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let encode = hex::encode(self.0);
        encode.fmt(f)
    }
}

#[derive(Debug)]
pub struct SignatureBuilder {
    bytes: Vec<u8>,
}

impl SignatureBuilder {
    pub fn new() -> Self {
        Self { bytes: vec![] }
    }

    pub fn from(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn finalize(self) -> Vec<u8> {
        self.bytes
    }
}

pub trait SignatureSource {
    fn write_bytes(&self, builder: &mut SignatureBuilder);

    fn build_signature_source(&self) -> Vec<u8> {
        let mut builder = SignatureBuilder::new();
        self.write_bytes(&mut builder);
        builder.finalize()
    }
}

impl<T> SignatureSource for &[T]
where
    T: SignatureSource,
{
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        for item in self.iter() {
            item.write_bytes(builder);
        }
    }
}
