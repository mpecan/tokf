//! Unit tests for `crate::filter::tree::apply_tree`.
//!
//! Lives in a sibling file rather than `#[cfg(test)] mod tests` inside
//! `tree.rs` so the implementation file stays under the 500-line soft
//! limit. All tests exercise the public API.

use super::tree::apply_tree;
use tokf_common::config::tree::{TreeConfig, TreeStyle};

fn cfg(pattern: &str) -> TreeConfig {
    TreeConfig {
        pattern: pattern.to_string(),
        passthrough_unmatched: true,
        min_files: 1,
        min_shared_depth: 0,
        style: TreeStyle::Unicode,
        collapse_single_child: false,
        sort: false,
    }
}

const fn git_pattern() -> &'static str {
    r"^(.. )(.+)$"
}

#[test]
fn collapse_basic() {
    let mut c = cfg(git_pattern());
    c.collapse_single_child = true;
    let lines = vec!["M  src/foo.rs", "M  src/bar.rs", "?? src/baz.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    // src/ has 3 children, no single-child collapse to do at the dir level.
    // Top-level src/ renders flush left, children get connectors.
    assert_eq!(
        out,
        vec![
            "src/".to_string(),
            "├─ M  foo.rs".to_string(),
            "├─ M  bar.rs".to_string(),
            "└─ ?? baz.rs".to_string(),
        ]
    );
}

#[test]
fn no_collapse_when_disabled() {
    let mut c = cfg(git_pattern());
    c.collapse_single_child = false;
    // Single chain: a/b/foo.rs and a/b/bar.rs — without collapse, two
    // dir levels render explicitly.
    let lines = vec!["M  a/b/foo.rs", "M  a/b/bar.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    assert_eq!(
        out,
        vec![
            "a/".to_string(),
            "└─ b/".to_string(),
            "   ├─ M  foo.rs".to_string(),
            "   └─ M  bar.rs".to_string(),
        ]
    );
}

#[test]
fn single_child_collapse() {
    let mut c = cfg(git_pattern());
    c.collapse_single_child = true;
    let lines = vec!["M  src/lib/foo.rs", "M  src/lib/bar.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    // src/ has one child lib/, which has two children — collapse src→lib
    assert_eq!(
        out,
        vec![
            "src/lib/".to_string(),
            "├─ M  foo.rs".to_string(),
            "└─ M  bar.rs".to_string(),
        ]
    );
}

#[test]
fn multi_root_fallback_when_min_shared_depth_required() {
    let mut c = cfg(git_pattern());
    c.min_shared_depth = 1;
    // Divergent roots: src/foo.rs and docs/bar.md — shared depth = 0
    let lines = vec!["M  src/foo.rs", "M  docs/bar.md", "?? README.md"];
    let out = apply_tree(&c, &lines);
    assert!(
        out.is_none(),
        "should fall back to flat when min_shared_depth not met"
    );
}

#[test]
fn empty_input() {
    let c = cfg(git_pattern());
    let out = apply_tree(&c, &[]);
    assert!(out.is_none(), "empty input → no engagement");
}

#[test]
fn no_match_input_passthrough() {
    let mut c = cfg(git_pattern());
    c.passthrough_unmatched = true;
    c.min_files = 1;
    // No lines match the git pattern (no XY-code prefix)
    let lines = vec!["just text", "more text"];
    let out = apply_tree(&c, &lines);
    // 0 matched → below min_files=1 → fallback
    assert!(out.is_none());
}

#[test]
fn mixed_matched_unmatched_header_stays_top() {
    let c = cfg(git_pattern());
    let lines = vec!["main [synced]", "M  src/a.rs", "M  src/b.rs", "M  src/c.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    assert_eq!(out[0], "main [synced]");
    assert_eq!(out[1], "src/");
    assert_eq!(out[2], "├─ M  a.rs");
    assert_eq!(out[3], "├─ M  b.rs");
    assert_eq!(out[4], "└─ M  c.rs");
}

#[test]
fn ordering_preserved() {
    let mut c = cfg(git_pattern());
    c.sort = false;
    // Reverse-alphabetical input — output should preserve source order.
    let lines = vec!["M  src/zeta.rs", "M  src/alpha.rs", "M  src/middle.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    assert_eq!(out[1], "├─ M  zeta.rs");
    assert_eq!(out[2], "├─ M  alpha.rs");
    assert_eq!(out[3], "└─ M  middle.rs");
}

#[test]
fn sort_alphabetical_when_opted_in() {
    let mut c = cfg(git_pattern());
    c.sort = true;
    let lines = vec!["M  src/zeta.rs", "M  src/alpha.rs", "M  src/middle.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    assert_eq!(out[1], "├─ M  alpha.rs");
    assert_eq!(out[2], "├─ M  middle.rs");
    assert_eq!(out[3], "└─ M  zeta.rs");
}

#[test]
fn min_files_fallback() {
    let mut c = cfg(git_pattern());
    c.min_files = 4;
    // Only 3 matched lines → below min_files
    let lines = vec!["M  src/a.rs", "M  src/b.rs", "M  src/c.rs"];
    let out = apply_tree(&c, &lines);
    assert!(out.is_none());
}

#[test]
fn min_shared_depth_fallback() {
    let mut c = cfg(git_pattern());
    c.min_shared_depth = 1;
    // 5 matched lines but no common prefix
    let lines = vec!["M  a.rs", "M  b.rs", "M  c.rs", "M  d.rs", "M  e.rs"];
    let out = apply_tree(&c, &lines);
    assert!(out.is_none());
}

#[test]
fn min_files_inclusive_boundary() {
    // Pin the inclusive boundary on min_files: with min_files = 3 and exactly
    // 3 matched lines, the tree MUST engage. Catches an off-by-one (`<` vs `<=`).
    let mut c = cfg(git_pattern());
    c.min_files = 3;
    let lines = vec!["M  src/a.rs", "M  src/b.rs", "M  src/c.rs"];
    let out = apply_tree(&c, &lines);
    assert!(out.is_some(), "exactly min_files matched lines must engage");
}

#[test]
fn min_shared_depth_inclusive_boundary() {
    // Pin the inclusive boundary on min_shared_depth: with min_shared_depth = 1
    // and a single shared prefix component, the tree MUST engage.
    let mut c = cfg(git_pattern());
    c.min_shared_depth = 1;
    c.min_files = 2;
    let lines = vec!["M  src/foo.rs", "M  src/bar.rs"];
    let out = apply_tree(&c, &lines);
    assert!(
        out.is_some(),
        "exactly min_shared_depth shared components must engage"
    );
}

#[test]
fn unicode_style() {
    let mut c = cfg(git_pattern());
    c.style = TreeStyle::Unicode;
    let lines = vec!["M  src/a.rs", "M  src/b.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    let joined = out.join("\n");
    assert!(joined.contains("├─"));
    assert!(joined.contains("└─"));
}

#[test]
fn ascii_style() {
    let mut c = cfg(git_pattern());
    c.style = TreeStyle::Ascii;
    let lines = vec!["M  src/a.rs", "M  src/b.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    let joined = out.join("\n");
    assert!(joined.contains("|-"));
    assert!(joined.contains("`-"));
    assert!(!joined.contains("├"));
}

#[test]
fn indent_style() {
    let mut c = cfg(git_pattern());
    c.style = TreeStyle::Indent;
    let lines = vec!["M  src/a.rs", "M  src/b.rs"];
    let out = apply_tree(&c, &lines).unwrap();
    // No connector chars at all — pure indent
    for line in &out[1..] {
        assert!(!line.contains('├'));
        assert!(!line.contains('└'));
        assert!(!line.contains('|'));
        assert!(!line.contains('`'));
    }
}

#[test]
fn rename_arrow_attached_to_leaf() {
    // Issue edge case 7: `R  old.rs -> new.rs` — the " -> new.rs" stays
    // attached to the leaf and doesn't break path component splitting.
    let c = cfg(git_pattern());
    let lines = vec![
        "R  src/old.rs -> src/new.rs",
        "M  src/other.rs",
        "M  src/third.rs",
    ];
    let out = apply_tree(&c, &lines).unwrap();
    // First file uses src/old.rs as the trie key, with " -> src/new.rs" tail
    let joined = out.join("\n");
    assert!(joined.contains("R  old.rs -> src/new.rs"), "got:\n{joined}");
    // No spurious " -> src" component in the tree
    assert!(!joined.contains("├─ -> "));
}

#[test]
fn invalid_regex_returns_none() {
    let mut c = cfg("[invalid");
    c.min_files = 1;
    let lines = vec!["M  src/a.rs"];
    let out = apply_tree(&c, &lines);
    assert!(out.is_none());
}

#[test]
fn multi_root_renders_when_no_min_shared_depth() {
    let mut c = cfg(git_pattern());
    c.min_shared_depth = 0; // no shared depth required
    c.collapse_single_child = true;
    let lines = vec![
        "M  crates/cli/main.rs",
        "M  crates/cli/lib.rs",
        "M  docs/getting-started.md",
        "M  README.md",
    ];
    let out = apply_tree(&c, &lines).unwrap();
    // crates/cli/ at top, docs/getting-started.md collapsed leaf,
    // README.md as plain top-level leaf.
    assert_eq!(out[0], "crates/cli/");
    assert_eq!(out[1], "├─ M  main.rs");
    assert_eq!(out[2], "└─ M  lib.rs");
    assert_eq!(out[3], "M  docs/getting-started.md");
    assert_eq!(out[4], "M  README.md");
}
