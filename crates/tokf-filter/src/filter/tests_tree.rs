//! End-to-end pipeline tests for the `[tree]` transform.
//!
//! Unit tests for `tree::apply_tree()` itself live in
//! `crates/tokf-filter/src/filter/tree.rs`. The tests here exercise the
//! integration into `apply_internal` — config parsing, pipeline ordering,
//! and interaction with surrounding stages (skip, dedup, branches).

use super::*;
use crate::CommandResult;

fn make_result(combined: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined: combined.to_string(),
    }
}

#[test]
fn tree_engages_on_path_list() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
min_files = 3
min_shared_depth = 1
"#,
    )
    .unwrap();
    let combined = "M  src/a.rs\nM  src/b.rs\nM  src/c.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    assert!(out.contains("src/"), "expected tree dir line, got:\n{out}");
    assert!(
        out.contains("├─"),
        "expected unicode connector, got:\n{out}"
    );
}

#[test]
fn tree_falls_back_below_min_files() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
min_files = 4
min_shared_depth = 0
"#,
    )
    .unwrap();
    // Only 2 matched lines — below min_files=4, should be flat
    let combined = "M  src/a.rs\nM  src/b.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    assert!(!out.contains("├─"), "should not engage, got:\n{out}");
    assert!(out.contains("M  src/a.rs"));
    assert!(out.contains("M  src/b.rs"));
}

#[test]
fn tree_runs_after_dedup() {
    // Two duplicate lines should be collapsed by dedup before tree sees them.
    // Without dedup, 4 lines would engage tree; with dedup → 3 unique lines,
    // still engages tree (min_files=3).
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
dedup = true
[tree]
pattern = '^(.. )(.+)$'
min_files = 3
min_shared_depth = 1
"#,
    )
    .unwrap();
    let combined = "M  src/a.rs\nM  src/a.rs\nM  src/b.rs\nM  src/c.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    // Each filename should appear exactly once
    assert_eq!(out.matches("a.rs").count(), 1, "got:\n{out}");
}

#[test]
fn tree_preserves_unmatched_header_lines() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
min_files = 3
min_shared_depth = 1
"#,
    )
    .unwrap();
    // First line doesn't match the pattern → should be preserved at the top
    let combined = "main [synced]\nM  src/a.rs\nM  src/b.rs\nM  src/c.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    let first_line = out.lines().next().unwrap();
    assert_eq!(first_line, "main [synced]");
    assert!(out.contains("src/"));
    assert!(out.contains("├─"));
}

#[test]
fn tree_absent_when_section_not_declared() {
    // Sanity: existing filters with no [tree] section behave exactly as before.
    let config: FilterConfig = toml::from_str(r#"command = "test""#).unwrap();
    let combined = "M  src/a.rs\nM  src/b.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    assert_eq!(out, "M  src/a.rs\nM  src/b.rs");
}

#[test]
fn tree_runs_after_skip() {
    // skip should remove a line before tree sees it.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^IGNORE"]
[tree]
pattern = '^(.. )(.+)$'
min_files = 3
min_shared_depth = 1
"#,
    )
    .unwrap();
    let combined = "IGNORE this line\nM  src/a.rs\nM  src/b.rs\nM  src/c.rs\nM  src/d.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    assert!(!out.contains("IGNORE"));
    assert!(out.contains("src/"));
    assert!(out.contains("├─"));
}

#[test]
fn tree_composes_with_on_success_output_template() {
    // The branch render runs after tree; on_success.output = "{output}"
    // should pass the tree-rendered text through verbatim.
    let config: FilterConfig = toml::from_str(
        r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
min_files = 3
min_shared_depth = 1

[on_success]
output = "{output}"
"#,
    )
    .unwrap();
    let combined = "M  src/a.rs\nM  src/b.rs\nM  src/c.rs\n";
    let result = make_result(combined, 0);
    let out = apply(&config, &result, &[], &FilterOptions::default()).output;
    assert!(out.contains("src/"));
    assert!(out.contains("├─"));
}
