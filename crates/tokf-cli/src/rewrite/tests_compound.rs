#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

use super::*;

// --- rewrite_with_config (compound commands) ---

#[test]
fn rewrite_compound_both_segments_match() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git add foo && git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "tokf run git add foo && tokf run git status");
}

#[test]
fn rewrite_compound_partial_match() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "unknown-cmd && git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "unknown-cmd && tokf run git status");
}

#[test]
fn rewrite_pipe_head_stripped_when_filter_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git diff HEAD | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // Simple pipe to head stripped — tokf filter provides structured output.
    assert_eq!(r, "tokf run --baseline-pipe 'head -5' git diff HEAD");
}

#[test]
fn rewrite_pipe_grep_stripped_when_filter_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | grep FAILED",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "tokf run --baseline-pipe 'grep FAILED' cargo test");
}

#[test]
fn rewrite_pipe_no_filter_preserves_pipe() {
    let dir = TempDir::new().unwrap();
    // No filter for "unknown-cmd"
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "unknown-cmd | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "unknown-cmd | tail -5");
}

#[test]
fn rewrite_pipe_wc_passes_through() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git status | wc -l",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // wc is not a strippable target — pipe passes through.
    assert_eq!(r, "git status | wc -l");
}

#[test]
fn rewrite_pipe_tail_follow_passes_through() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | tail -f",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // tail -f (follow mode) is not strippable.
    assert_eq!(r, "cargo test | tail -f");
}

#[test]
fn rewrite_logical_or_still_rewritten() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test || echo failed",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "tokf run cargo test || echo failed");
}

#[test]
fn rewrite_multi_pipe_chain_not_rewritten() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git status | grep M | wc -l",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "git status | grep M | wc -l");
}

#[test]
fn rewrite_compound_no_match_passthrough() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "unknown-a && unknown-b",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "unknown-a && unknown-b");
}

#[test]
fn rewrite_user_rule_wraps_piped_command() {
    // User-configured rules run before the pipe guard, so they CAN wrap piped commands.
    // Using {0}{rest} captures both the matched portion and the remainder (including the pipe).
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^cargo test".to_string(),
            replace: "my-wrapper {0}{rest}".to_string(),
        }],
    };
    let r = rewrite_with_config(
        "cargo test | grep FAILED",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "my-wrapper cargo test| grep FAILED");
}

#[test]
fn rewrite_skip_pattern_wins_over_pipe_guard() {
    // should_skip is checked first; a piped command that matches a skip pattern
    // returns early before the pipe guard even runs.
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: Some(types::SkipConfig {
            patterns: vec!["^git".to_string()],
        }),
        pipe: None,
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "git status | grep M",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "git status | grep M");
}

#[test]
fn rewrite_compound_then_pipe_stripped() {
    // A compound command where one segment has a strippable pipe: each segment
    // is rewritten independently, and the pipe in the second segment is stripped.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git add . && git diff | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run git add . && tokf run --baseline-pipe 'head -5' git diff"
    );
}

#[test]
fn rewrite_quoted_pipe_is_not_a_bare_pipe() {
    // A pipe inside quotes is not a shell pipe operator — the command should be rewritten.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("grep.toml"), "command = \"grep\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "grep -E 'foo|bar' file.txt",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // No bare pipe — the filter rule should fire.
    assert_eq!(r, "tokf run grep -E 'foo|bar' file.txt");
}

#[test]
fn rewrite_pipe_grep_quoted_pattern_escaped() {
    // Single quotes in the grep suffix must be escaped with '\'' in the
    // --baseline-pipe value so the shell command remains valid.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | grep -E 'fail|error'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run --baseline-pipe 'grep -E '\\''fail|error'\\''' cargo test"
    );
}
