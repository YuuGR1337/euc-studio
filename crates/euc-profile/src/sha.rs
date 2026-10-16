use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::Digest;
use thiserror::Error;

/// A SHA-256 digest, written as 64 lowercase hex digits.
///
/// Stock firmware is identified only by this value: the repo never holds vendor bytes, so a pin in
/// `wheel.toml` is the whole proof that a downloaded or imported file is the official build.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Sha256([u8; 32]);

impl Sha256 {
    /// Hashes `bytes`.
    pub fn digest(bytes: &[u8]) -> Self {
        Self(sha2::Sha256::digest(bytes).into())
    }

    /// First 8 and last 4 hex digits (`e3b0c442…b855`), enough to tell builds apart on screen.
    pub fn short(&self) -> String {
        let hex = self.to_string();
        format!("{}…{}", &hex[..8], &hex[60..])
    }
}

/// A string that is not 64 hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{0}` is not a sha256 (expected 64 hex digits)")]
pub struct BadSha256(pub String);

impl FromStr for Sha256 {
    type Err = BadSha256;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0; 32];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| BadSha256(s.to_owned()))?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({self})")
    }
}

impl Serialize for Sha256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of the empty string (FIPS 180-4 test vector).
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn digest_matches_the_test_vector() {
        assert_eq!(Sha256::digest(b"").to_string(), EMPTY);
    }

    #[test]
    fn parses_either_case_and_prints_lowercase() {
        let sha: Sha256 = EMPTY.to_uppercase().parse().unwrap();
        assert_eq!(sha, Sha256::digest(b""));
        assert_eq!(sha.to_string(), EMPTY);
        assert_eq!(sha.short(), "e3b0c442…b855");
    }

    #[test]
    fn rejects_bad_text() {
        for bad in ["", "e3b0", &EMPTY[1..], &format!("{EMPTY}00"), &EMPTY.replace('e', "g")] {
            assert_eq!(bad.parse::<Sha256>(), Err(BadSha256(bad.to_owned())));
        }
    }
}
