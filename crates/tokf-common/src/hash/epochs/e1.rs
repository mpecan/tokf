//! Hash epoch **e1** — first stable `canonical_hash` schema.
//!
//! Source of truth: `git show 5abfaf8:crates/tokf-common/src/config/types.rs`
//! (commit `5abfaf8`, 2026-02-22, "feat(filter): canonical content hash for
//! filter identity (#126)"). That commit introduced `canonical_hash` and
//! switched `GroupConfig.labels` from `HashMap` → `BTreeMap`, making the
//! JSON serialisation order-stable.
//!
//! ## FROZEN — DO NOT MODIFY
//!
//! Any change to the structs in this module — adding a field, removing a
//! field, changing a `#[derive]`, changing a `#[serde]` annotation, even
//! reordering fields — silently invalidates every `e1:…` hash ever
//! computed. The frozen-corpus CI test under
//! `crates/tokf-common/tests/hash_corpus/e1/` catches most violations but
//! not all. If the underlying schema needs to change, that's a *new*
//! epoch (`e2`, `e3`, …), not an edit of this one.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::hash::HashError;

const VERSION: &str = "e1";

/// Compute the e1 hash for a filter TOML.
///
/// Parses `toml_str` into the e1-frozen `FilterConfig`, serialises via
/// `serde_json::to_vec` (struct declaration order — same as the binary at
/// commit `5abfaf8`), SHA-256s the bytes, and prefixes `"e1:"`.
///
/// # Errors
///
/// - `HashError` if the TOML is malformed for the e1 shape, or JSON
///   serialisation fails (the latter should not happen for any
///   well-formed parse).
pub fn hash(toml_str: &str) -> Result<String, HashError> {
    let cfg: schema::FilterConfig = toml::from_str(toml_str)?;
    let json = serde_json::to_vec(&cfg)?;
    let digest = Sha256::digest(&json);
    let mut out = String::with_capacity(VERSION.len() + 1 + 64);
    let _ = write!(out, "{VERSION}:");
    for b in &digest {
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────
// FROZEN SCHEMA SNAPSHOT — verbatim copy of types.rs at commit 5abfaf8.
// Any modification below silently invalidates every published e1:… hash.
// ─────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
mod schema {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum CommandPattern {
        Single(String),
        Multiple(Vec<String>),
    }

    impl Default for CommandPattern {
        fn default() -> Self {
            Self::Single(String::new())
        }
    }

    #[allow(clippy::struct_excessive_bools)]
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FilterConfig {
        pub command: CommandPattern,
        pub run: Option<String>,
        #[serde(default)]
        pub skip: Vec<String>,
        #[serde(default)]
        pub keep: Vec<String>,
        #[serde(default)]
        pub step: Vec<Step>,
        pub extract: Option<ExtractRule>,
        #[serde(default)]
        pub match_output: Vec<MatchOutputRule>,
        #[serde(default)]
        pub section: Vec<Section>,
        pub on_success: Option<OutputBranch>,
        pub on_failure: Option<OutputBranch>,
        pub parse: Option<ParseConfig>,
        pub output: Option<OutputConfig>,
        pub fallback: Option<FallbackConfig>,
        #[serde(default)]
        pub replace: Vec<ReplaceRule>,
        #[serde(default)]
        pub dedup: bool,
        pub dedup_window: Option<usize>,
        #[serde(default)]
        pub strip_ansi: bool,
        #[serde(default)]
        pub trim_lines: bool,
        #[serde(default)]
        pub strip_empty_lines: bool,
        #[serde(default)]
        pub collapse_empty_lines: bool,
        #[serde(default)]
        pub lua_script: Option<ScriptConfig>,
        #[serde(default)]
        pub variant: Vec<Variant>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Step {
        pub run: String,
        #[serde(rename = "as")]
        pub as_name: Option<String>,
        pub pipeline: Option<bool>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ExtractRule {
        pub pattern: String,
        pub output: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct MatchOutputRule {
        pub contains: String,
        pub output: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Section {
        pub name: Option<String>,
        pub enter: Option<String>,
        pub exit: Option<String>,
        #[serde(rename = "match")]
        pub match_pattern: Option<String>,
        pub split_on: Option<String>,
        pub collect_as: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct OutputBranch {
        pub output: Option<String>,
        pub aggregate: Option<AggregateRule>,
        pub tail: Option<usize>,
        pub head: Option<usize>,
        #[serde(default)]
        pub skip: Vec<String>,
        pub extract: Option<ExtractRule>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct AggregateRule {
        pub from: String,
        pub pattern: String,
        pub sum: Option<String>,
        pub count_as: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ParseConfig {
        pub branch: Option<LineExtract>,
        pub group: Option<GroupConfig>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct LineExtract {
        pub line: usize,
        pub pattern: String,
        pub output: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct GroupConfig {
        pub key: ExtractRule,
        #[serde(default)]
        pub labels: BTreeMap<String, String>,
    }

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct OutputConfig {
        pub format: Option<String>,
        pub group_counts_format: Option<String>,
        pub empty: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct FallbackConfig {
        pub tail: Option<usize>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ReplaceRule {
        pub pattern: String,
        pub output: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum ScriptLang {
        Luau,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ScriptConfig {
        pub lang: ScriptLang,
        pub file: Option<String>,
        pub source: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct VariantDetect {
        #[serde(default)]
        pub files: Vec<String>,
        pub output_pattern: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct Variant {
        pub name: String,
        pub detect: VariantDetect,
        pub filter: String,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn output_is_versioned_hex() {
        let h = hash(r#"command = "git push""#).unwrap();
        assert!(h.starts_with("e1:"));
        assert_eq!(h.len(), 3 + 64);
        assert!(
            h[3..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    /// e1 was the first STABLE schema specifically because labels became a
    /// `BTreeMap`. Reordering label keys in the source TOML must not change
    /// the hash.
    #[test]
    fn label_key_order_invariance() {
        let a = hash(
            r#"
command = "git status"
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
            r#"
command = "git status"
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

    /// TOML with fields that didn't exist at e1 (e.g. `inject_path`,
    /// `show_history_hint`) must hash identically to TOML without them.
    /// Serde silently drops unknown fields, exactly matching what the
    /// binary at commit 5abfaf8 would have done.
    #[test]
    fn unknown_fields_are_silently_dropped() {
        let with_new = hash(
            r#"
command = "git push"
inject_path = true
show_history_hint = true
"#,
        )
        .unwrap();
        let without_new = hash(r#"command = "git push""#).unwrap();
        assert_eq!(with_new, without_new);
    }

    /// Same TOML, varied whitespace and comments → same hash. Comments and
    /// formatting are erased by `toml::from_str` before serialisation, so
    /// they don't contribute to the canonical hash. This is the same
    /// invariant `current::canonical_hash` advertises.
    #[test]
    fn whitespace_and_comments_invariant() {
        let a = hash(r#"command = "git push""#).unwrap();
        let b = hash("# leading comment\ncommand    =    \"git push\"\n\n").unwrap();
        assert_eq!(a, b);
    }

    /// Malformed TOML must surface as `HashError::Parse`, not panic.
    #[test]
    fn malformed_toml_returns_parse_error() {
        let err = hash("this is = not = valid = toml = at all").unwrap_err();
        assert!(
            matches!(err, HashError::Parse(_)),
            "expected Parse, got {err:?}"
        );
    }

    /// Frozen reference vector. The expected value is captured ONCE at
    /// authoring time and never changed; if this test fails, the schema
    /// snapshot has drifted and a new epoch must be created instead of
    /// editing e1. The corpus under
    /// `crates/tokf-common/tests/hash_corpus/e1/` is the broader
    /// equivalent; this is the inline smoke check.
    #[test]
    fn frozen_reference_minimal_filter() {
        let h = hash(r#"command = "git push""#).unwrap();
        assert_eq!(
            h, "e1:2c7b698282f042f3e391f54743c292357a679019220a31ff763d81150f21798d",
            "e1 schema has drifted; bump to e2 instead of editing e1"
        );
    }
}
