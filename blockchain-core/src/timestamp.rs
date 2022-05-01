use crate::signature::{SignatureBuilder, SignatureSource};
use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::hash::{Hash, Hasher};

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

impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl SignatureSource for Timestamp {
    fn write_bytes(&self, builder: &mut SignatureBuilder) {
        builder.write_bytes(&self.0.naive_utc().timestamp_nanos().to_le_bytes());
    }
}
