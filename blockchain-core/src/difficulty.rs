use super::digest::BlockDigest;
use crate::signature::{SignatureBuilder, SignatureSource};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Difficulty(u8);

impl Difficulty {
    pub const fn new(difficulty: u8) -> Self {
        Self(difficulty)
    }

    pub fn raise(self) -> Self {
        Self(self.0.checked_add(1).unwrap_or(u8::MAX))
    }

    pub fn ease(self) -> Self {
        Self(self.0.checked_sub(1).unwrap_or_default())
    }

    pub fn verify_digest(&self, digest: &BlockDigest) -> bool {
        self.verify_bytes(digest.as_ref())
    }

    fn verify_bytes(&self, bytes: &[u8]) -> bool {
        count_first_0_bits(bytes) >= self.0
    }
}

impl SignatureSource for Difficulty {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&[self.0]);
    }
}

fn count_first_0_bits(bytes: &[u8]) -> u8 {
    let mut count = 0;

    for &byte in bytes {
        match count_first_0_bit(byte) {
            8 => count += 8,
            c => {
                count += c;
                break;
            }
        }
    }

    count
}

const fn count_first_0_bit(x: u8) -> u8 {
    let mut count = 0;
    let mut flag = 1 << (8 - 1);
    while flag > 0 {
        if x & flag == 0 {
            count += 1;
            flag >>= 1;
        } else {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::Difficulty;

    #[test]
    fn test_difficulty_zero() {
        let d = Difficulty(0);

        assert!(d.verify_bytes(&[255]));
    }

    #[test]
    fn test_difficulty_one() {
        let d = Difficulty(1);

        assert!(d.verify_bytes(&[127]));
        assert!(!d.verify_bytes(&[128]));
    }

    #[test]
    fn test_difficulty_8() {
        let d = Difficulty(8);

        assert!(d.verify_bytes(&[0]));
        assert!(!d.verify_bytes(&[1]));
    }

    #[test]
    fn test_difficulty_9() {
        let d = Difficulty(9);

        assert!(d.verify_bytes(&[0, 127]));
        assert!(!d.verify_bytes(&[0, 128]));
        assert!(!d.verify_bytes(&[0]));
        assert!(!d.verify_bytes(&[1]));
    }
}
