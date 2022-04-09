use crate::signature::{SignatureBuilder, SignatureSource};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn enix_epoch() -> Self {
        let timestamp = NaiveDateTime::from_timestamp(0, 0);
        let datetime = DateTime::from_utc(timestamp, Utc);
        Self(datetime)
    }
}

impl Hash for Timestamp {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.0.naive_utc().hash(state);
    }
}

impl Sub for Timestamp {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        let duration = self.0 - rhs.0;
        Duration(duration)
    }
}

impl SignatureSource for Timestamp {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&self.0.naive_utc().timestamp_nanos().to_le_bytes());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration(chrono::Duration);

impl Add for Duration {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}
