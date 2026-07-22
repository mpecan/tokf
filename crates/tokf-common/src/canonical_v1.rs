//! Canonical v1 filter hash.
//!
//! Implements the algorithm specified in `docs/adr/0002-canonical-v1-hash.md`:
//!
//! 1. Parse TOML 1.0 to a [`toml::Value`] tree.
//! 2. Walk the tree applying three normalisation passes:
//!    - sort arrays whose paths appear in [`UNORDERED_PATHS`];
//!    - collapse `command = ["x"]` to `command = "x"` (single-entry only);
//!    - prune entries equal to `false`, `[]`, or `{}` (TOML-level defaults).
//! 3. Re-emit via [`toml::to_string`].
//! 4. SHA-256 the bytes; format as `v1:<lowercase-hex>`.
//!
//! v1 is FROZEN. Modifying this module's behaviour for an existing input
//! is a v2 trigger, not a fix. The frozen corpus under
//! `crates/tokf-common/tests/canonical_v1_corpus/` is the contract.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use toml::Value;

const VERSION: &str = "v1";

/// Paths whose values are unordered arrays (sets) under v1's policy table.
///
/// Matched structurally against the TOML AST. A path like
/// `"on_success.skip"` matches the `skip` key inside the `on_success`
/// table; `"skip"` matches the top-level `skip` key. New paths added here
/// must use the conservative semantics — once shipped under a v1 hash,
/// the policy is frozen for that path. See ADR-0002 §"Adding to the
/// table".
const UNORDERED_PATHS: &[&str] = &["skip", "keep", "on_success.skip", "on_failure.skip"];

/// Compute the canonical v1 hash of a filter TOML.
///
/// # Errors
///
/// Returns a [`HashError`] if the input is not valid TOML 1.0, the root
/// is not a table, or any float is non-finite.
pub fn hash(toml_str: &str) -> Result<String, HashError> {
    let mut value: Value = toml::from_str(toml_str)?;
    let table = value.as_table_mut().ok_or(HashError::RootNotTable)?;
    reject_non_finite_floats_in_table(table)?;

    sort_unordered_arrays(table, "");
    collapse_command_single_form(table);
    prune_defaults_in_table(table);

    let canonical = toml::to_string(&Value::Table(std::mem::take(table)))
        .map_err(|e| HashError::Emit(e.to_string()))?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut out = String::with_capacity(VERSION.len() + 1 + 64);
    let _ = write!(out, "{VERSION}:");
    for b in &digest {
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}

#[derive(Debug)]
pub enum HashError {
    /// Input was not valid TOML 1.0.
    Parse(String),
    /// Input parsed but the root was not a table.
    RootNotTable,
    /// Input contained `inf`, `-inf`, or `nan`.
    NonFiniteFloat,
    /// `toml::to_string` failed on the canonical value (should not happen
    /// for any well-formed parse; defensive).
    Emit(String),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(m) => write!(f, "parse: {m}"),
            Self::RootNotTable => write!(f, "filter root must be a TOML table"),
            Self::NonFiniteFloat => write!(f, "filter contains a non-finite float (inf or nan)"),
            Self::Emit(m) => write!(f, "canonical TOML emission failed: {m}"),
        }
    }
}

impl std::error::Error for HashError {}

impl From<toml::de::Error> for HashError {
    fn from(e: toml::de::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

// ── Normalisation passes ──────────────────────────────────────────────────

fn reject_non_finite_floats_in_table(t: &toml::map::Map<String, Value>) -> Result<(), HashError> {
    for v in t.values() {
        reject_non_finite_floats_in_value(v)?;
    }
    Ok(())
}

fn reject_non_finite_floats_in_value(v: &Value) -> Result<(), HashError> {
    match v {
        Value::Float(f) if !f.is_finite() => Err(HashError::NonFiniteFloat),
        Value::Array(a) => a.iter().try_for_each(reject_non_finite_floats_in_value),
        Value::Table(t) => reject_non_finite_floats_in_table(t),
        _ => Ok(()),
    }
}

/// Sort arrays whose path appears in [`UNORDERED_PATHS`]. `path_prefix` is
/// the dotted path to `t` from the document root (`""` at root).
fn sort_unordered_arrays(t: &mut toml::map::Map<String, Value>, path_prefix: &str) {
    for (key, value) in t.iter_mut() {
        let here = if path_prefix.is_empty() {
            key.clone()
        } else {
            format!("{path_prefix}.{key}")
        };
        match value {
            Value::Array(arr) if UNORDERED_PATHS.contains(&here.as_str()) => {
                sort_array_canonically(arr);
            }
            Value::Array(arr) => {
                // Walk array-of-tables to find unordered paths nested inside.
                for item in arr.iter_mut() {
                    if let Value::Table(inner) = item {
                        sort_unordered_arrays(inner, &here);
                    }
                }
            }
            Value::Table(inner) => sort_unordered_arrays(inner, &here),
            _ => {}
        }
    }
}

/// Sort array elements by their canonical TOML byte representation,
/// byte-wise. Stable: equal elements preserve their relative order.
fn sort_array_canonically(arr: &mut [Value]) {
    arr.sort_by_cached_key(|v| {
        // Wrapping in a single-key table makes any value emittable, including
        // bare scalars.
        let mut wrapper = toml::map::Map::new();
        wrapper.insert("v".to_string(), v.clone());
        toml::to_string(&Value::Table(wrapper)).unwrap_or_default()
    });
}

/// Replace `command = ["x"]` with `command = "x"` at the document root.
/// Per ADR-0002 §"Special form: command".
fn collapse_command_single_form(t: &mut toml::map::Map<String, Value>) {
    if let Some(Value::Array(arr)) = t.get("command")
        && arr.len() == 1
        && let Some(Value::String(s)) = arr.first()
    {
        let single = Value::String(s.clone());
        t.insert("command".to_string(), single);
    }
}

/// Recursively drop entries equal to `false`, `[]`, or `{}`. Returns
/// whether the surrounding container itself becomes empty after pruning.
fn prune_defaults_in_table(t: &mut toml::map::Map<String, Value>) -> bool {
    let keys: Vec<String> = t.keys().cloned().collect();
    for key in keys {
        let drop = match t.get_mut(&key) {
            Some(Value::Boolean(false)) => true,
            Some(Value::Array(arr)) => {
                for item in arr.iter_mut() {
                    if let Value::Table(inner) = item {
                        // Don't prune array-of-tables entries: their position
                        // is semantically meaningful.
                        prune_defaults_in_table(inner);
                    }
                }
                arr.is_empty()
            }
            Some(Value::Table(inner)) => prune_defaults_in_table(inner),
            _ => false,
        };
        if drop {
            t.remove(&key);
        }
    }
    t.is_empty()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn output_format_is_v1_prefix_plus_64_hex() {
        let h = hash(r#"command = "git push""#).unwrap();
        assert!(h.starts_with("v1:"));
        assert_eq!(h.len(), 3 + 64);
        assert!(
            h[3..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn deterministic_across_calls() {
        let toml = r#"command = "git push""#;
        assert_eq!(hash(toml).unwrap(), hash(toml).unwrap());
    }

    #[test]
    fn whitespace_and_comments_invariant() {
        let a = hash(r#"command = "git push""#).unwrap();
        let b = hash("# leading comment\ncommand    =    \"git push\"\n\n").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn skip_is_unordered() {
        let a = hash(
            r#"command = "x"
skip = ["aaa", "bbb"]"#,
        )
        .unwrap();
        let b = hash(
            r#"command = "x"
skip = ["bbb", "aaa"]"#,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn keep_is_unordered() {
        let a = hash(
            r#"command = "x"
keep = ["aaa", "bbb"]"#,
        )
        .unwrap();
        let b = hash(
            r#"command = "x"
keep = ["bbb", "aaa"]"#,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn nested_skip_in_on_success_is_unordered() {
        let a = hash(
            r#"command = "x"
[on_success]
skip = ["aaa", "bbb"]"#,
        )
        .unwrap();
        let b = hash(
            r#"command = "x"
[on_success]
skip = ["bbb", "aaa"]"#,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn step_is_ordered() {
        let a = hash(
            r#"command = "x"
[[step]]
run = "a"
[[step]]
run = "b""#,
        )
        .unwrap();
        let b = hash(
            r#"command = "x"
[[step]]
run = "b"
[[step]]
run = "a""#,
        )
        .unwrap();
        assert_ne!(a, b, "[[step]] must preserve source order");
    }

    #[test]
    fn replace_is_ordered() {
        let a = hash(
            r#"command = "x"
[[replace]]
pattern = "a"
output = "1"
[[replace]]
pattern = "b"
output = "2""#,
        )
        .unwrap();
        let b = hash(
            r#"command = "x"
[[replace]]
pattern = "b"
output = "2"
[[replace]]
pattern = "a"
output = "1""#,
        )
        .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn command_single_and_array_of_one_collapse() {
        let a = hash(r#"command = "git push""#).unwrap();
        let b = hash(r#"command = ["git push"]"#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn command_array_of_two_does_not_collapse() {
        let single = hash(r#"command = "git""#).unwrap();
        let multi = hash(r#"command = ["git", "git push"]"#).unwrap();
        assert_ne!(single, multi);
    }

    #[test]
    fn command_array_order_matters_when_multiple() {
        let a = hash(r#"command = ["aa", "bb"]"#).unwrap();
        let b = hash(r#"command = ["bb", "aa"]"#).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn dedup_false_omitted() {
        let a = hash(
            r#"command = "x"
dedup = false"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn empty_array_omitted() {
        let a = hash(
            r#"command = "x"
skip = []"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn empty_table_omitted() {
        let a = hash(
            r#"command = "x"
[on_success]"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn nested_table_emptied_after_pruning_is_omitted() {
        // on_success contains only default-valued fields → after pruning
        // it's an empty table → it should itself be pruned.
        let a = hash(
            r#"command = "x"
[on_success]
skip = []
"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn dedup_true_preserved() {
        let a = hash(
            r#"command = "x"
dedup = true"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn zero_integer_preserved() {
        let a = hash(
            r#"command = "x"
dedup_window = 0"#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_ne!(a, b, "0 is a meaningful value, distinct from absent");
    }

    #[test]
    fn empty_string_preserved() {
        let a = hash(
            r#"command = "x"
run = """#,
        )
        .unwrap();
        let b = hash(r#"command = "x""#).unwrap();
        assert_ne!(a, b, "empty string is a meaningful value");
    }

    #[test]
    fn parse_group_labels_btreemap_invariant() {
        let a = hash(
            r#"command = "git status"
[parse.group.key]
pattern = "^(.{2}) "
output = "{1}"
[parse.group.labels]
M = "modified"
A = "added"
"#,
        )
        .unwrap();
        let b = hash(
            r#"command = "git status"
[parse.group.key]
pattern = "^(.{2}) "
output = "{1}"
[parse.group.labels]
A = "added"
M = "modified"
"#,
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn schema_independent_unknown_field_changes_hash_only_when_used() {
        // A field tokf-common doesn't know about: appearing in TOML changes
        // the hash; absent has no effect.
        let with_field = hash(
            r#"command = "x"
some_future_field = "configured""#,
        )
        .unwrap();
        let without = hash(r#"command = "x""#).unwrap();
        assert_ne!(with_field, without);
    }

    #[test]
    fn alias_renames_change_hash() {
        // Document the canonical-form-not-semantic-form rule: serde aliases
        // produce different canonical bytes, hence different hashes.
        let canonical_name = hash(
            r#"command = "x"
skip = ["foo"]"#,
        )
        .unwrap();
        let alias_name = hash(
            r#"command = "x"
strip_lines_matching = ["foo"]"#,
        )
        .unwrap();
        assert_ne!(canonical_name, alias_name);
    }

    #[test]
    fn rejects_non_finite_float() {
        let result = hash(
            r#"command = "x"
ratio = nan"#,
        );
        assert!(
            matches!(result, Err(HashError::NonFiniteFloat)),
            "got {result:?}"
        );
    }

    #[test]
    fn rejects_invalid_toml() {
        let result = hash("not = valid = toml = at = all");
        assert!(matches!(result, Err(HashError::Parse(_))), "got {result:?}");
    }

    /// Frozen reference: the v1 hash for the smallest valid filter must
    /// equal this value, forever. If this test fails, v1's behaviour has
    /// drifted — investigate before changing the expected value.
    #[test]
    fn frozen_reference_minimal_filter() {
        let h = hash(r#"command = "git push""#).unwrap();
        assert_eq!(
            h, "v1:ca4d87a2bc16ccee27b7007660b810ac02abeb0c917f99ab26b30497d6e52164",
            "v1 schema has drifted — this is the canary, do not just update the expected value"
        );
    }
}
