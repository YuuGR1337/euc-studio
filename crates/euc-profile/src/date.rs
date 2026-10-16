use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A calendar date written as a bare TOML date (`date = 2025-11-01`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// Year, e.g. 2025.
    pub year: u16,
    /// Month, 1 to 12.
    pub month: u8,
    /// Day of the month, 1 to 31.
    pub day: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = toml::value::Datetime::deserialize(deserializer)?;
        match value {
            toml::value::Datetime { date: Some(date), time: None, offset: None } => {
                Ok(Self { year: date.year, month: date.month, day: date.day })
            }
            other => Err(de::Error::custom(format!("expected a plain date such as 2025-11-01, found {other}"))),
        }
    }
}
