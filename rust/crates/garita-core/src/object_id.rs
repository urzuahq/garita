//! A git object name: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ObjectId(String);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{0}` is not a git object hash (40 or 64 lowercase hex digits)")]
pub struct ObjectIdError(pub String);

impl ObjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ObjectId {
    type Err = ObjectIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if hex && (s.len() == 40 || s.len() == 64) {
            Ok(Self(s.to_string()))
        } else {
            Err(ObjectIdError(s.to_string()))
        }
    }
}

impl TryFrom<String> for ObjectId {
    type Error = ObjectIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ObjectId> for String {
    fn from(id: ObjectId) -> Self {
        id.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sha1_and_sha256_names() {
        let sha1 = "9c1e4f2ab07d5e3c8f1a6b4d2e0c9f7a5b3d1e80";
        assert_eq!(sha1.parse::<ObjectId>().unwrap().as_str(), sha1);
        assert!("a".repeat(64).parse::<ObjectId>().is_ok());
    }

    #[test]
    fn rejects_anything_else() {
        for bad in [
            "",
            "abc",
            &"A".repeat(40),
            &"g".repeat(40),
            &"a".repeat(41),
            &"a".repeat(63),
        ] {
            assert_eq!(bad.parse::<ObjectId>(), Err(ObjectIdError(bad.into())));
        }
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let id: ObjectId = "a".repeat(40).parse().unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", "a".repeat(40)));
        assert_eq!(serde_json::from_str::<ObjectId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<ObjectId>("\"xyz\"").is_err());
    }
}
