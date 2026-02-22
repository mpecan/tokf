use sha2::{Digest, Sha256};

use crate::config::types::FilterConfig;

/// Error returned when a [`FilterConfig`] cannot be hashed.
///
/// Wraps the underlying serialization error without exposing `serde_json` as
/// a public dependency of this crate.
#[derive(Debug)]
pub struct HashError(serde_json::Error);

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for HashError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<serde_json::Error> for HashError {
    fn from(e: serde_json::Error) -> Self {
        Self(e)
    }
}

/// Compute a deterministic SHA-256 content hash for a [`FilterConfig`].
///
/// Two configs that are logically identical (same fields, same values) produce
/// the same hash regardless of TOML whitespace or key ordering, because the
/// hash is derived from canonical JSON serialization.
///
/// # Errors
///
/// Returns a [`HashError`] if `config` cannot be serialized to JSON (should
/// not happen for well-formed `FilterConfig` values, but callers must handle
/// it).
pub fn canonical_hash(config: &FilterConfig) -> Result<String, HashError> {
    let json = serde_json::to_vec(config)?;
    let digest = Sha256::digest(&json);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> FilterConfig {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn output_is_64_lowercase_hex_chars() {
        let cfg = parse(r#"command = "git push""#);
        let hash = canonical_hash(&cfg).unwrap();
        assert_eq!(hash.len(), 64, "hash must be 64 chars");
        assert!(
            hash.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
            "hash must be lowercase hex: {hash}"
        );
    }

    #[test]
    fn whitespace_invariance() {
        let a = parse(r#"command = "git push""#);
        let b = parse("command    =    \"git push\"\n\n");
        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn label_key_order_invariance() {
        // GroupConfig lives at FilterConfig.parse.group — use the correct TOML
        // path. The key field is ExtractRule { pattern, output } (no `line`).
        let a = parse(
            r#"
command = "git status"

[parse.group.key]
pattern = "^(.{2}) "
output = "{1}"

[parse.group.labels]
M = "modified"
A = "added"
"#,
        );
        let b = parse(
            r#"
command = "git status"

[parse.group.key]
pattern = "^(.{2}) "
output = "{1}"

[parse.group.labels]
A = "added"
M = "modified"
"#,
        );
        assert_eq!(
            canonical_hash(&a).unwrap(),
            canonical_hash(&b).unwrap(),
            "label key ordering must not affect hash"
        );
    }

    #[test]
    fn different_configs_produce_different_hashes() {
        let a = parse(r#"command = "git push""#);
        let b = parse(r#"command = "git pull""#);
        assert_ne!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn hash_is_stable_across_calls() {
        let cfg = parse(r#"command = "cargo build""#);
        let h1 = canonical_hash(&cfg).unwrap();
        let h2 = canonical_hash(&cfg).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn explicit_defaults_same_as_implicit() {
        // A minimal config relies on serde defaults for all optional fields.
        // A config that explicitly sets every default value to its zero/empty
        // equivalent must produce the same hash — proving that #[serde(default)]
        // fields are handled consistently.
        let implicit = parse(r#"command = "git push""#);
        let explicit = parse(
            r#"
command = "git push"
skip = []
keep = []
step = []
match_output = []
section = []
replace = []
variant = []
dedup = false
strip_ansi = false
trim_lines = false
strip_empty_lines = false
collapse_empty_lines = false
"#,
        );
        assert_eq!(
            canonical_hash(&implicit).unwrap(),
            canonical_hash(&explicit).unwrap(),
            "explicit defaults must hash identically to implicit defaults"
        );
    }
}
