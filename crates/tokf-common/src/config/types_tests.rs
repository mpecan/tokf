#![allow(clippy::unwrap_used)]

use super::tree::TreeStyle;
use super::types::{FilterConfig, MatchOutputRule};

#[test]
fn match_output_validate_accepts_contains() {
    let rule = MatchOutputRule {
        contains: Some("error".to_string()),
        pattern: None,
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_accepts_pattern() {
    let rule = MatchOutputRule {
        contains: None,
        pattern: Some("error".to_string()),
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_accepts_both() {
    let rule = MatchOutputRule {
        contains: Some("error".to_string()),
        pattern: Some("err.*".to_string()),
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_rejects_neither() {
    let rule = MatchOutputRule {
        contains: None,
        pattern: None,
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_err());
}

#[test]
fn tree_config_parses_minimal() {
    let toml = r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
"#;
    let cfg: FilterConfig = toml::from_str(toml).unwrap();
    let tree = cfg.tree.unwrap();
    assert_eq!(tree.pattern, "^(.. )(.+)$");
    // Defaults from tree.rs
    assert!(tree.passthrough_unmatched);
    assert_eq!(tree.min_files, 4);
    assert_eq!(tree.min_shared_depth, 1);
    assert_eq!(tree.style, TreeStyle::Unicode);
    assert!(tree.collapse_single_child);
    assert!(!tree.sort);
}

#[test]
fn tree_config_parses_full_overrides() {
    let toml = r#"
command = "git status"
[tree]
pattern = '^(M  )(.+)$'
passthrough_unmatched = false
min_files = 8
min_shared_depth = 2
style = "ascii"
collapse_single_child = false
sort = true
"#;
    let cfg: FilterConfig = toml::from_str(toml).unwrap();
    let tree = cfg.tree.unwrap();
    assert!(!tree.passthrough_unmatched);
    assert_eq!(tree.min_files, 8);
    assert_eq!(tree.min_shared_depth, 2);
    assert_eq!(tree.style, TreeStyle::Ascii);
    assert!(!tree.collapse_single_child);
    assert!(tree.sort);
}

#[test]
fn tree_config_round_trips_through_json() {
    // The cache layer (crates/tokf-cli/src/config/cache.rs) serializes
    // FilterConfig as JSON, so any new field must round-trip cleanly.
    let toml = r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
style = "indent"
"#;
    let cfg: FilterConfig = toml::from_str(toml).unwrap();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: FilterConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg.tree, back.tree);
    let tree = back.tree.unwrap();
    assert_eq!(tree.style, TreeStyle::Indent);
}

#[test]
fn tree_config_absent_when_not_declared() {
    let toml = r#"command = "git status""#;
    let cfg: FilterConfig = toml::from_str(toml).unwrap();
    assert!(cfg.tree.is_none());
}

#[test]
fn tree_config_rejects_unknown_field() {
    let toml = r#"
command = "git status"
[tree]
pattern = '^(.. )(.+)$'
this_field_does_not_exist = true
"#;
    let result: Result<FilterConfig, _> = toml::from_str(toml);
    assert!(
        result.is_err(),
        "deny_unknown_fields should reject unknown keys"
    );
}
