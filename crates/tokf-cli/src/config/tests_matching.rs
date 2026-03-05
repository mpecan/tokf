#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::needless_collect
)]

use super::*;

// --- pattern_specificity ---

#[test]
fn specificity_two_literals() {
    assert_eq!(pattern_specificity("git push"), 2);
}

#[test]
fn specificity_wildcard_counts_less() {
    assert_eq!(pattern_specificity("git *"), 1);
    assert_eq!(pattern_specificity("* push"), 1);
}

#[test]
fn specificity_all_wildcards() {
    assert_eq!(pattern_specificity("* *"), 0);
}

#[test]
fn specificity_ordering() {
    // "git push" more specific than "git *" more specific than "* push"
    assert!(pattern_specificity("git push") > pattern_specificity("git *"));
    assert!(pattern_specificity("git *") == pattern_specificity("* push"));
}

// --- pattern_matches_prefix ---

#[test]
fn matches_exact() {
    let words = ["git", "push"];
    assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
}

#[test]
fn matches_prefix_with_trailing_args() {
    let words = ["git", "push", "origin", "main"];
    assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
}

#[test]
fn matches_wildcard() {
    let words = ["npm", "run", "build"];
    assert_eq!(pattern_matches_prefix("npm run *", &words), Some(3));
}

#[test]
fn no_match_different_command() {
    let words = ["cargo", "test"];
    assert_eq!(pattern_matches_prefix("git push", &words), None);
}

#[test]
fn no_match_too_short() {
    let words = ["git"];
    assert_eq!(pattern_matches_prefix("git push", &words), None);
}

#[test]
fn empty_pattern_returns_none() {
    let words = ["git", "push"];
    assert_eq!(pattern_matches_prefix("", &words), None);
}

#[test]
fn empty_words_returns_none() {
    assert_eq!(pattern_matches_prefix("git push", &[]), None);
}

#[test]
fn single_word_pattern_prefix_match() {
    assert_eq!(pattern_matches_prefix("echo", &["echo"]), Some(1));
    assert_eq!(pattern_matches_prefix("echo", &["echo", "hello"]), Some(1));
    assert_eq!(pattern_matches_prefix("echo", &["ls"]), None);
}

#[test]
fn wildcard_rejects_empty_token() {
    // An empty string slice element is not a valid word match for `*`
    assert_eq!(pattern_matches_prefix("git *", &["git", ""]), None);
}

#[test]
fn wildcard_at_start() {
    let words = ["my-tool", "subcommand"];
    assert_eq!(pattern_matches_prefix("* subcommand", &words), Some(2));
}

#[test]
fn hyphenated_tool_not_ambiguous() {
    // golangci-lint run should match "golangci-lint run" but not "golangci-lint"
    let words = ["golangci-lint", "run"];
    assert_eq!(pattern_matches_prefix("golangci-lint run", &words), Some(2));
    assert_eq!(pattern_matches_prefix("golangci-lint", &words), Some(1));
}

#[test]
fn basename_matching() {
    assert_eq!(pattern_matches_prefix("ls", &["/usr/bin/ls"]), Some(1));
    assert_eq!(
        pattern_matches_prefix("ls -la", &["/usr/bin/ls", "-la"]),
        Some(2)
    );
    assert_eq!(pattern_matches_prefix("mvnw", &["./mvnw"]), Some(1));
    assert_eq!(
        pattern_matches_prefix("git push", &["git", "push"]),
        Some(2)
    );
    assert_eq!(pattern_matches_prefix("git /p", &["git", "/p"]), Some(2));
    assert_eq!(pattern_matches_prefix("git f", &["git", "/p"]), None);
}

// --- extract_basename ---

#[test]
fn basename_plain_word() {
    assert_eq!(extract_basename("git"), "git");
}

#[test]
fn basename_absolute_path() {
    assert_eq!(extract_basename("/usr/bin/git"), "git");
}

#[test]
fn basename_relative_path() {
    assert_eq!(extract_basename("./mvnw"), "mvnw");
}

#[test]
fn basename_windows_path() {
    assert_eq!(extract_basename(r"C:\project\mvnw"), "mvnw");
}

#[test]
fn basename_trailing_separator_returns_empty() {
    // e.g. a path accidentally ending in /
    assert_eq!(extract_basename("git/"), "");
}

#[test]
fn basename_root_returns_empty() {
    assert_eq!(extract_basename("/"), "");
}

#[test]
fn basename_empty_string() {
    assert_eq!(extract_basename(""), "");
}

// --- skip_flags_to_match ---

#[test]
fn skip_flags_short_flag_with_value() {
    // -C /path: flag + separate value, then target
    let words = ["-C", "/path", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_long_flag_no_value() {
    // --no-pager is a standalone flag (next word is target)
    let words = ["--no-pager", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_long_flag_equals_value() {
    // --format=%s: entire token is flag+value
    let words = ["--format=%s", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_multiple_flags() {
    let words = ["--no-pager", "-C", "/path", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(4));
}

#[test]
fn skip_flags_target_first() {
    // target is the very first word — consumed immediately
    let words = ["log", "--oneline"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(1));
}

#[test]
fn skip_flags_non_flag_non_target_blocks() {
    // "something" is not a flag and not the target → None
    let words = ["something", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), None);
}

#[test]
fn skip_flags_empty_words() {
    assert_eq!(skip_flags_to_match(&[], "log"), None);
}

#[test]
fn skip_flags_flag_value_is_target() {
    // -C log: "/path" slot is occupied by "log" itself — it IS the target,
    // so we should NOT skip it as a value.
    // Words: ["-C", "log", "log"] — first "log" should be detected as target.
    let words = ["-C", "log", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_combined_form_no_space() {
    // -Cpath is treated as a single flag token (no separate value), then target
    let words = ["-Cpath", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_flag_value_looks_like_flag() {
    // -C -v: since -v starts with '-', it is NOT consumed as -C's value —
    // it is treated as the next standalone flag instead.
    let words = ["-C", "-v", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_multiple_equals_style_flags() {
    // Two --flag=value tokens in a row, then target
    let words = ["--fmt=%s", "--depth=5", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_flag_only_at_end_no_target() {
    // Flag at end of slice, target never found
    let words = ["-C"];
    assert_eq!(skip_flags_to_match(&words, "log"), None);
}

// --- transparent global args in pattern_matches_prefix ---

#[test]
fn transparent_short_flag_with_value() {
    // git -C /path log  →  matches "git log", consumed = 4
    let words = ["git", "-C", "/path", "log", "--oneline"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(4));
}

#[test]
fn transparent_long_flag_no_value() {
    // git --no-pager log  →  consumed = 3
    let words = ["git", "--no-pager", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(3));
}

#[test]
fn transparent_flag_equals_value() {
    // git --format=%s log  →  consumed = 3
    let words = ["git", "--format=%s", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(3));
}

#[test]
fn transparent_multiple_flags() {
    // git --no-pager -C /path log  →  consumed = 5
    let words = ["git", "--no-pager", "-C", "/path", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(5));
}

#[test]
fn transparent_no_skip_for_non_flags() {
    // "somedir" is not a flag — should not match
    let words = ["git", "somedir", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), None);
}

#[test]
fn transparent_direct_match_unchanged() {
    // No transparent args — existing behaviour preserved
    let words = ["git", "log", "--oneline"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(2));
}

#[test]
fn transparent_combined_with_basename() {
    // /usr/bin/git -C /path log  →  basename match + transparent skip
    let words = ["/usr/bin/git", "-C", "/path", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(4));
}

#[test]
fn transparent_wildcard_with_flags_before_literal() {
    // npm --prefix /path run build  →  pattern "npm run *", consumed = 5
    let words = ["npm", "--prefix", "/path", "run", "build"];
    assert_eq!(pattern_matches_prefix("npm run *", &words), Some(5));
}

#[test]
fn transparent_flags_not_skipped_at_first_word() {
    // Transparent skipping only happens after the first word
    // A command starting with a flag should not match
    let words = ["--no-pager", "git", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), None);
}

// --- command_pattern_to_regex ---

#[test]
fn regex_from_literal_pattern() {
    let r = command_pattern_to_regex("git push");
    let re = regex::Regex::new(&r).unwrap();
    // Basic matches (existing behaviour preserved)
    assert!(re.is_match("git push"));
    assert!(re.is_match("git push origin main"));
    assert!(!re.is_match("git status"));
}

#[test]
fn regex_from_wildcard_pattern() {
    let r = command_pattern_to_regex("npm run *");
    let re = regex::Regex::new(&r).unwrap();
    // Basic matches (existing behaviour preserved)
    assert!(re.is_match("npm run build"));
    assert!(re.is_match("npm run test --watch"));
    assert!(!re.is_match("npm run"));
    assert!(!re.is_match("npm install"));
}

#[test]
fn regex_basename_matching() {
    let r = command_pattern_to_regex("git push");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("/usr/bin/git push"), "full path should match");
    assert!(re.is_match("./git push"), "relative path should match");
    assert!(!re.is_match("git-lfs push"), "git-lfs ≠ git");
}

#[test]
fn regex_transparent_short_flag_with_value() {
    // git -C /path log  →  matches "git log"
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git -C /path log"));
    assert!(re.is_match("git -C /path log --oneline"));
}

#[test]
fn regex_transparent_long_flag_no_value() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --no-pager log"));
    assert!(re.is_match("git --no-pager log --oneline"));
}

#[test]
fn regex_transparent_flag_equals_value() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --format=%s log"));
}

#[test]
fn regex_transparent_multiple_flags() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --no-pager -C /path log"));
    assert!(re.is_match("git --no-pager -C /path log --oneline"));
}

#[test]
fn regex_transparent_combined_with_basename() {
    // /usr/bin/git -C /path log  →  matches "git log"
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("/usr/bin/git -C /path log"));
    assert!(re.is_match("/usr/bin/git --no-pager -C /path log --oneline"));
}

#[test]
fn regex_non_flag_between_words_no_match() {
    // A non-flag, non-target word between pattern words should NOT match
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(
        !re.is_match("git somedir log"),
        "non-flag should block match"
    );
}

#[test]
fn regex_transparent_wildcard_with_flags_before_run() {
    // npm --prefix /path run build  →  matches "npm run *"
    let r = command_pattern_to_regex("npm run *");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("npm --prefix /path run build"));
}

#[test]
fn regex_empty_pattern() {
    // Degenerate: empty pattern should produce a valid regex
    let r = command_pattern_to_regex("");
    assert!(regex::Regex::new(&r).is_ok());
}
