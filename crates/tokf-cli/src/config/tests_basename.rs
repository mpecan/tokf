#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::needless_collect
)]

use super::*;

// ============================================================
// pattern_matches_prefix — path-prefixed patterns (both sides)
// ============================================================

#[test]
fn pattern_path_prefix_same_path() {
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["./mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn pattern_path_prefix_pattern_has_path_input_bare() {
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn pattern_path_prefix_different_relative_path() {
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["../mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn pattern_path_prefix_deep_relative_path() {
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["../some/mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn pattern_bare_matches_path_input() {
    assert_eq!(
        pattern_matches_prefix("mvnw test", &["../mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn pattern_path_prefix_reversed_paths() {
    assert_eq!(
        pattern_matches_prefix("../mvnw test", &["./mvnw", "test"]),
        Some(2)
    );
}

// --- absolute path in pattern ---

#[test]
fn pattern_absolute_path_prefix() {
    assert_eq!(
        pattern_matches_prefix("/usr/local/bin/git push", &["git", "push"]),
        Some(2)
    );
    assert_eq!(
        pattern_matches_prefix("/usr/local/bin/git push", &["/usr/bin/git", "push"]),
        Some(2)
    );
}

// --- single-word path pattern (no subcommand) ---

#[test]
fn pattern_path_only_no_subcommand() {
    assert_eq!(pattern_matches_prefix("./mvnw", &["mvnw"]), Some(1));
    assert_eq!(pattern_matches_prefix("./mvnw", &["mvnw", "test"]), Some(1));
}

#[test]
fn pattern_absolute_path_only_no_subcommand() {
    assert_eq!(pattern_matches_prefix("/usr/bin/ls", &["ls"]), Some(1));
    assert_eq!(
        pattern_matches_prefix("/usr/bin/ls", &["ls", "-la"]),
        Some(1)
    );
}

// --- wildcard at position 0 with path inputs ---

#[test]
fn wildcard_first_word_with_path_input() {
    assert_eq!(
        pattern_matches_prefix("* push", &["/usr/bin/git", "push"]),
        Some(2)
    );
    assert_eq!(
        pattern_matches_prefix("* push", &["./git", "push"]),
        Some(2)
    );
}

// --- empty basename edge cases ---

#[test]
fn basename_empty_after_strip_no_match() {
    // Input "/", basename is "" — should not match "git"
    assert_eq!(pattern_matches_prefix("git push", &["/", "push"]), None);
}

#[test]
fn basename_trailing_slash_no_match() {
    // Input "git/", basename is "" — should not match "git"
    assert_eq!(pattern_matches_prefix("git push", &["git/", "push"]), None);
}

// --- negative: partial / prefix / suffix basename ---

#[test]
fn basename_partial_name_no_match() {
    assert_eq!(
        pattern_matches_prefix("git push", &["/usr/bin/gi", "push"]),
        None
    );
}

#[test]
fn basename_suffix_no_match() {
    assert_eq!(pattern_matches_prefix("git push", &["xgit", "push"]), None);
    assert_eq!(
        pattern_matches_prefix("git push", &["/usr/bin/xgit", "push"]),
        None
    );
}

#[test]
fn basename_different_tool_same_path_depth() {
    assert_eq!(
        pattern_matches_prefix("git push", &["/usr/bin/hg", "push"]),
        None
    );
}

// --- path-prefixed pattern + transparent flag skipping ---

#[test]
fn path_prefixed_pattern_with_transparent_flags() {
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["mvnw", "--debug", "test"]),
        Some(3)
    );
    assert_eq!(
        pattern_matches_prefix("./mvnw test", &["../mvnw", "-X", "val", "test"]),
        Some(4)
    );
}

// --- Windows paths in pattern_matches_prefix ---

#[test]
fn basename_matching_windows_input() {
    assert_eq!(
        pattern_matches_prefix("mvnw test", &[r"C:\project\mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn basename_matching_windows_pattern() {
    assert_eq!(
        pattern_matches_prefix(r"C:\project\mvnw test", &["mvnw", "test"]),
        Some(2)
    );
}

#[test]
fn basename_matching_windows_both() {
    assert_eq!(
        pattern_matches_prefix(r"C:\tools\mvnw test", &[r"D:\project\mvnw", "test"]),
        Some(2)
    );
}

// ============================================================
// command_pattern_to_regex — path-prefixed patterns
// ============================================================

#[test]
fn regex_path_prefixed_pattern() {
    let r = command_pattern_to_regex("./mvnw test");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("mvnw test"), "bare input should match");
    assert!(
        re.is_match("./mvnw test"),
        "same relative path should match"
    );
    assert!(
        re.is_match("../mvnw test"),
        "parent-relative path should match"
    );
    assert!(
        re.is_match("../some/mvnw test"),
        "deep relative path should match"
    );
    assert!(
        !re.is_match("./mvnw-wrapper test"),
        "different basename should not match"
    );
}

// --- normalization equivalence ---

#[test]
fn regex_path_pattern_normalized_to_bare() {
    let r1 = command_pattern_to_regex("./mvnw test");
    let r2 = command_pattern_to_regex("mvnw test");
    assert_eq!(r1, r2, "path-prefixed pattern should produce same regex");
}

#[test]
fn regex_absolute_path_pattern_normalized() {
    let r1 = command_pattern_to_regex("/usr/local/bin/git push");
    let r2 = command_pattern_to_regex("git push");
    assert_eq!(
        r1, r2,
        "absolute-path pattern should produce same regex as bare"
    );
}

// --- negative regex cases ---

#[test]
fn regex_basename_negative_cases() {
    let r = command_pattern_to_regex("git push");
    let re = regex::Regex::new(&r).unwrap();
    assert!(!re.is_match("xgit push"), "prefix mismatch");
    assert!(!re.is_match("/usr/bin/hg push"), "different basename");
    assert!(!re.is_match("gitx push"), "suffix mismatch");
}
