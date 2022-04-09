use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(ed25519::Signature);

impl Signature {
    pub fn from(sign: ed25519::Signature) -> Self {
        Self(sign)
    }
}

impl Hash for Signature {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.0.as_ref().hash(state);
    }
}

impl AsRef<ed25519::Signature> for Signature {
    fn as_ref(&self) -> &ed25519::Signature {
        &self.0
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
