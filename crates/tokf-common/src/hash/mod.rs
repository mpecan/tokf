//! Versioned canonical hashes for filter content.
//!
//! Two layers, composed:
//!
//! 1. **`current`** — the schema-tied `canonical_hash(&FilterConfig)`. Used
//!    at publish time and anywhere that already has a parsed `FilterConfig`.
//!    Its output drifts with `FilterConfig` schema additions (issue #350) —
//!    new fields with `#[serde(default)]` silently change every filter's
//!    hash.
//!
//! 2. **`epochs`** — frozen byte-for-byte snapshots of the `FilterConfig`
//!    schema at specific commits, each with its own hash function that
//!    reproduces exactly what the binary at that commit would have produced.
//!    Clients try every known epoch; whichever matches the URL hash
//!    verifies the content. Each epoch is FROZEN once shipped; new schemas
//!    become new epochs (`e2`, `e3`, …) rather than edits to existing ones.
//!
//! See `epochs/e1.rs` for the canonical pattern.

pub mod current;
mod epochs;
mod error;

pub use current::canonical_hash;
pub use error::HashError;

/// A registered hash version: a stable identifier and the function that
/// computes its hash from raw filter TOML.
///
/// Hashers must not panic; any failure mode (parse error, schema
/// mismatch, serialisation issue) must be reported as a [`HashError`].
/// `compute_all` and `matches_any` drop `Err` results silently but do not
/// catch panics.
#[derive(Debug, Clone, Copy)]
pub struct HashVersion {
    /// Stable identifier used as the hash prefix (e.g. `"e1"`).
    pub id: &'static str,
    hasher: fn(&str) -> Result<String, HashError>,
}

impl HashVersion {
    /// Compute this version's hash for `toml`.
    ///
    /// # Errors
    ///
    /// Returns a [`HashError`] if `toml` cannot be parsed under this
    /// version's frozen schema or serialised to JSON.
    pub fn hash(self, toml: &str) -> Result<String, HashError> {
        (self.hasher)(toml)
    }
}

/// All known versioned hash schemes, listed in the order clients should try
/// them when matching a stored hash. Earlier entries are checked first.
///
/// Subsequent epochs (`e2`, `e3`, …) are added to this slice as they ship;
/// the order is maintained to put the most-likely match first for the
/// current generation of filters in the wild.
pub const KNOWN_VERSIONS: &[HashVersion] = &[HashVersion {
    id: "e1",
    hasher: epochs::e1::hash,
}];

/// Compute every known versioned hash for `toml`.
///
/// Errors per version are dropped silently — a version may legitimately
/// fail (e.g. malformed TOML for that epoch's shape); we just exclude it
/// from the result.
//
// TODO(#350-followup): wire into the install flow's `verify_and_resolve_hash`
// (PR #351 path) once that branch is merged so clients can verify URL
// hashes against any known epoch without a server round-trip.
pub fn compute_all(toml: &str) -> Vec<(&'static str, String)> {
    KNOWN_VERSIONS
        .iter()
        .filter_map(|v| v.hash(toml).ok().map(|h| (v.id, h)))
        .collect()
}

/// Find the first known epoch (if any) whose hash equals `expected`.
///
/// Returns the epoch's `id` on match, `None` if no version matches. Used
/// by the install flow as a fast-path before falling back to server-trust
/// verification.
pub fn matches_any(toml: &str, expected: &str) -> Option<&'static str> {
    KNOWN_VERSIONS
        .iter()
        .find_map(|v| v.hash(toml).ok().filter(|h| h == expected).map(|_| v.id))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn known_versions_is_non_empty() {
        // Sanity: KNOWN_VERSIONS must include at least one epoch so
        // `compute_all`/`matches_any` are actually useful. If this fails,
        // someone removed every epoch — verify they meant to.
        assert!(!KNOWN_VERSIONS.is_empty(), "no hash versions registered");
    }

    #[test]
    fn compute_all_returns_one_entry_per_version() {
        let result = compute_all(r#"command = "git push""#);
        assert_eq!(result.len(), KNOWN_VERSIONS.len());
        for (got, expected) in result.iter().zip(KNOWN_VERSIONS) {
            assert_eq!(got.0, expected.id);
        }
    }

    #[test]
    fn matches_any_finds_known_hash() {
        let toml = r#"command = "git push""#;
        let computed = KNOWN_VERSIONS[0].hash(toml).unwrap();
        assert_eq!(matches_any(toml, &computed), Some(KNOWN_VERSIONS[0].id));
    }

    #[test]
    fn matches_any_returns_none_for_unknown_hash() {
        assert_eq!(matches_any(r#"command = "git push""#, "e1:0000"), None);
    }

    /// `compute_all` and `matches_any` must silently drop versions whose
    /// hasher returns `Err` — they're best-effort lookups, not validators.
    #[test]
    fn malformed_toml_yields_no_matches() {
        let bad = "this = is = malformed = toml";
        assert!(compute_all(bad).is_empty());
        assert_eq!(matches_any(bad, "e1:anything"), None);
    }
}
