use crate::signature::{SignatureBuilder, SignatureSource};
use apply::{Also, Apply};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockDigest(#[serde(with = "serde_arrays")] [u8; 32]);

impl BlockDigest {
    pub fn digest(input: &[u8]) -> Self {
        Sha256::new()
            .also(|hasher| hasher.update(input))
            .apply(Sha256::finalize)
            .apply(|inner| Self(inner.into()))
    }
}

impl AsRef<[u8]> for BlockDigest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl SignatureSource for BlockDigest {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde() {
        let data = vec![42, 255, 0];
        let digest = BlockDigest::digest(&data);

        let ser = serde_json::to_string(&digest).unwrap();
        let de = serde_json::from_str::<BlockDigest>(&ser).unwrap();

        assert_eq!(digest, de);
    }

    #[test]
    fn test_serde_valid_length() {
        let data = [42_u8; 32];

        let ser = serde_json::to_string(&data).unwrap();
        let de = serde_json::from_str::<BlockDigest>(&ser);

        assert!(de.is_ok());
    }

    #[test]
    fn test_serde_invalid_length() {
        let data = [42_u8; 31]; // Too short digest!

        let ser = serde_json::to_string(&data).unwrap();
        let de = serde_json::from_str::<BlockDigest>(&ser);

        assert!(de.is_err());
    }
}
