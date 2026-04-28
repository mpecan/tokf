//! Error type for the hash module. Lives here so `hash::current` and
//! `hash::epochs::*` can share it without each duplicating wrapper boilerplate.

/// Error returned when a filter cannot be hashed.
///
/// Wraps the underlying serialisation/deserialisation error without
/// exposing `serde_json` or `toml` as public dependencies of this crate.
#[derive(Debug)]
pub enum HashError {
    /// Failed to parse filter TOML against an epoch's schema.
    Parse(String),
    /// Failed to JSON-serialise a parsed filter for hashing. Should not
    /// happen for any well-formed parse.
    Serialize(String),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "parse: {m}"),
            Self::Serialize(m) => write!(f, "serialize: {m}"),
        }
    }
}

impl std::error::Error for HashError {}

impl From<serde_json::Error> for HashError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e.to_string())
    }
}

impl From<toml::de::Error> for HashError {
    fn from(e: toml::de::Error) -> Self {
        Self::Parse(e.to_string())
    }
}
