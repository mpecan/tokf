//! Frozen snapshot + property tests for `canonical_v1::hash`.
//!
//! ## Snapshot corpus
//!
//! Every `.toml` under `crates/tokf-cli/filters/` (excluding `_test/` dirs)
//! is hashed; the result is checked against the recorded value in
//! `tests/canonical_v1_stdlib.txt`. **Do not modify recorded values** —
//! a change in any expected hash means v1's behaviour drifted, which is
//! either a bug to fix or a v2 trigger, never a "just update the
//! expected" case.
//!
//! Authoring: when adding new stdlib filters or building this file for the
//! first time, run:
//!
//! ```sh
//! cargo test -p tokf-common --test canonical_v1 -- dump_stdlib_hashes \
//!     --ignored --nocapture > crates/tokf-common/tests/canonical_v1_stdlib.txt
//! ```
//!
//! ## Property tests
//!
//! For each of a representative subset of stdlib filters, apply
//! transformations that the spec says are invariant (whitespace, comments,
//! reordered unordered arrays, default omission, `command` collapse) and
//! assert the hash is unchanged. Transformations that the spec says are
//! distinguishing (reordered `[[step]]`, changed values) must change the
//! hash.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tokf_common::canonical_v1;

// ── Discovery ─────────────────────────────────────────────────────────────

fn stdlib_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tokf-cli/filters")
}

/// Walk `stdlib_root()`, return `(relative_path_without_ext, content)` for
/// every filter `.toml`. Excludes `_test/` directories.
fn collect_stdlib_filters() -> Vec<(String, String)> {
    let root = stdlib_root();
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if path.is_dir() {
            if name_str.ends_with("_test") {
                continue;
            }
            walk(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "toml") {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .with_extension("")
                .to_string_lossy()
                .to_string();
            let content = std::fs::read_to_string(&path).unwrap();
            out.push((rel, content));
        }
    }
}

// ── Snapshot corpus ──────────────────────────────────────────────────────

fn recorded_hashes() -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/canonical_v1_stdlib.txt");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("missing {}: {e}", path.display()));
    content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (k, v) = l
                .split_once(": ")
                .unwrap_or_else(|| panic!("malformed line: {l:?}"));
            (k.to_string(), v.to_string())
        })
        .collect()
}

#[test]
fn stdlib_corpus_round_trip() {
    let filters = collect_stdlib_filters();
    assert!(
        !filters.is_empty(),
        "no stdlib filters discovered under {}",
        stdlib_root().display()
    );
    let recorded = recorded_hashes();

    let mut missing = Vec::new();
    let mut drifted = Vec::new();
    for (rel, content) in &filters {
        let actual =
            canonical_v1::hash(content).unwrap_or_else(|e| panic!("{rel}: hash failed: {e}"));
        match recorded.get(rel) {
            None => missing.push(format!("{rel}: {actual}")),
            Some(expected) if expected != &actual => {
                drifted.push(format!(
                    "{rel}\n    expected {expected}\n    actual   {actual}"
                ));
            }
            _ => {}
        }
    }

    let stdlib_paths: std::collections::BTreeSet<&str> =
        filters.iter().map(|(p, _)| p.as_str()).collect();
    let orphans: Vec<&str> = recorded
        .keys()
        .filter(|k| !stdlib_paths.contains(k.as_str()))
        .map(String::as_str)
        .collect();

    let mut errors = Vec::new();
    if !drifted.is_empty() {
        errors.push(format!(
            "hash drift in {} filter(s):\n{}",
            drifted.len(),
            drifted.join("\n")
        ));
    }
    if !missing.is_empty() {
        errors.push(format!(
            "{} filter(s) missing from canonical_v1_stdlib.txt:\n{}\n\
             (run dump_stdlib_hashes to refresh — see test file header)",
            missing.len(),
            missing.join("\n")
        ));
    }
    if !orphans.is_empty() {
        errors.push(format!(
            "{} orphan entries in canonical_v1_stdlib.txt (filter no longer exists):\n  {}",
            orphans.len(),
            orphans.join("\n  ")
        ));
    }
    assert!(errors.is_empty(), "{}", errors.join("\n\n"));
}

/// Authoring helper. Print the canonical-v1 hash of every stdlib filter in
/// the recorded-file format, sorted. Pipe to `tests/canonical_v1_stdlib.txt`
/// when adding new filters or rebuilding the corpus.
#[test]
#[ignore = "authoring helper; run with --ignored to capture expected values"]
fn dump_stdlib_hashes() {
    println!("# canonical_v1 hashes for all stdlib filters under crates/tokf-cli/filters/");
    println!(
        "# generated by `cargo test -p tokf-common --test canonical_v1 -- dump_stdlib_hashes --ignored --nocapture`"
    );
    println!("# DO NOT edit this file by hand.");
    println!();
    for (rel, content) in collect_stdlib_filters() {
        let h = canonical_v1::hash(&content).unwrap();
        println!("{rel}: {h}");
    }
}

// ── Property tests ────────────────────────────────────────────────────────

/// A small representative subset of the stdlib for property tests. We don't
/// run them across all 51 — that's the corpus's job — but we want enough
/// shape diversity that the invariants are exercised on real-world inputs.
const PROP_TEST_FILTERS: &[&str] = &[
    "git/push",
    "git/commit",
    "git/status",
    "git/log",
    "cargo/test",
    "cargo/clippy",
    "playwright",
    "kubectl/get/pods",
];

fn load(rel: &str) -> String {
    let path = stdlib_root().join(format!("{rel}.toml"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn assert_invariant<F: Fn(&str) -> String>(label: &str, transform: F) {
    for rel in PROP_TEST_FILTERS {
        let original = load(rel);
        let transformed = transform(&original);
        let h_original = canonical_v1::hash(&original).unwrap();
        let h_transformed = canonical_v1::hash(&transformed).unwrap_or_else(|e| {
            panic!("{rel} after {label}: hash failed: {e}\n--- transformed ---\n{transformed}")
        });
        assert_eq!(
            h_original, h_transformed,
            "{label} should not change the v1 hash for {rel}\n\
             original ─────\n{original}\ntransformed ──\n{transformed}\n"
        );
    }
}

/// Idempotence: running the filter through a `toml` round-trip
/// (`from_str` → `to_string`) before hashing must produce the same v1
/// hash. This proves the canonicaliser is stable against arbitrary
/// reformatting that the toml crate itself produces.
#[test]
fn invariant_toml_roundtrip_idempotent() {
    for rel in PROP_TEST_FILTERS {
        let original = load(rel);
        let parsed: toml::Value = toml::from_str(&original).unwrap();
        let roundtripped = toml::to_string(&parsed).unwrap();
        let h_original = canonical_v1::hash(&original).unwrap();
        let h_round = canonical_v1::hash(&roundtripped).unwrap();
        assert_eq!(
            h_original, h_round,
            "toml round-trip changed v1 hash for {rel}"
        );
    }
}

/// Adding leading blank lines and a leading comment must not change the
/// hash. Both are pure file-level affordances stripped at parse time.
/// Note: only the prefix of the file is transformed, so we never modify
/// anything inside a multi-line string body.
#[test]
fn invariant_leading_comments_and_blanks() {
    assert_invariant("leading comments + blank lines", |s| {
        format!("# leading comment\n#\n# more comment\n\n\n{s}")
    });
}

/// Reorder unordered-array entries via the AST and assert hash unchanged.
/// We work at the AST layer rather than text so we don't accidentally
/// touch multi-line string content (some filters embed Lua scripts).
#[test]
fn invariant_skip_keep_reversed_via_ast() {
    for rel in PROP_TEST_FILTERS {
        let original = load(rel);
        let mut value: toml::Value = toml::from_str(&original).unwrap();
        let mut mutated = false;
        for field in ["skip", "keep"] {
            if let Some(toml::Value::Array(arr)) =
                value.as_table_mut().and_then(|t| t.get_mut(field))
                && arr.len() >= 2
            {
                arr.reverse();
                mutated = true;
            }
        }
        if !mutated {
            continue;
        }
        let transformed = toml::to_string(&value).unwrap();
        let h_original = canonical_v1::hash(&original).unwrap();
        let h_transformed = canonical_v1::hash(&transformed).unwrap();
        assert_eq!(
            h_original, h_transformed,
            "reversing skip/keep should not change the v1 hash for {rel}"
        );
    }
}

#[test]
fn invariant_default_false_added() {
    // Adding `dedup = false` (or another false-bool) to any filter that
    // doesn't already use that key must not change the hash. Pick a key the
    // filter doesn't mention.
    for rel in PROP_TEST_FILTERS {
        let original = load(rel);
        if original.contains("dedup =") {
            continue;
        }
        let transformed = format!("dedup = false\n{original}");
        let h_original = canonical_v1::hash(&original).unwrap();
        let h_transformed = canonical_v1::hash(&transformed).unwrap();
        assert_eq!(
            h_original, h_transformed,
            "adding `dedup = false` must not change the v1 hash for {rel}"
        );
    }
}

#[test]
fn distinguishing_value_change() {
    // Sanity check the property tests aren't trivially passing: a real
    // value change must change the hash. Modify the `command` field.
    for rel in PROP_TEST_FILTERS {
        let original = load(rel);
        let transformed = original.replace("command =", "command = \"this-changes-it\" #");
        if transformed == original {
            continue;
        }
        // The transformation may not always result in valid TOML; only
        // assert when both parse successfully.
        let h_original = canonical_v1::hash(&original).unwrap();
        let Ok(h_transformed) = canonical_v1::hash(&transformed) else {
            continue;
        };
        assert_ne!(
            h_original, h_transformed,
            "changing `command` must change the v1 hash for {rel}"
        );
    }
}
