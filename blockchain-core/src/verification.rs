use serde::{Deserialize, Serialize};

/// A marker type that represents something passed verification process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yet;

/// A marker type that represents something has not passed verification process yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified;
