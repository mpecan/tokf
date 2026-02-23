//! Integration tests for environment variable prefix handling in the rewrite engine.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

use super::*;

#[test]
fn rewrite_single_env_var_prefix() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "DEBUG=1 git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "DEBUG=1 tokf run git status");
}

#[test]
fn rewrite_multiple_env_vars_prefix() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "RUST_LOG=debug CARGO_TERM_COLOR=always cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "RUST_LOG=debug CARGO_TERM_COLOR=always tokf run cargo test"
    );
}

#[test]
fn rewrite_env_var_with_strippable_pipe() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "RUST_LOG=debug cargo test | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "RUST_LOG=debug tokf run --baseline-pipe 'tail -5' cargo test"
    );
}

#[test]
fn rewrite_env_var_with_non_strippable_pipe_passthrough() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "DEBUG=1 git status | wc -l",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // wc is not strippable — entire command passes through unchanged.
    assert_eq!(r, "DEBUG=1 git status | wc -l");
}

#[test]
fn rewrite_env_var_no_filter_match_passthrough() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "FOO=bar unknown-cmd",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "FOO=bar unknown-cmd");
}

#[test]
fn rewrite_env_var_in_compound_command() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "DEBUG=1 git status && cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "DEBUG=1 tokf run git status && tokf run cargo test");
}

#[test]
fn rewrite_env_var_with_full_path_command() {
    // Env var prefix + basename matching: FOO=bar /usr/bin/git status.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "FOO=bar /usr/bin/git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "FOO=bar tokf run /usr/bin/git status");
}

#[test]
fn rewrite_env_var_with_transparent_global_flags() {
    // Env var prefix + transparent global flags: FOO=bar git -C /repo log.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-log.toml"), "command = \"git log\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "FOO=bar git -C /repo log --oneline",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "FOO=bar tokf run git -C /repo log --oneline");
}

#[test]
fn rewrite_compound_both_segments_have_env_vars() {
    // Both segments of a compound command carry their own env var prefix.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "A=1 git status && B=2 cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "A=1 tokf run git status && B=2 tokf run cargo test");
}

#[test]
fn rewrite_env_prefixed_tokf_command_not_double_wrapped() {
    // DEBUG=1 tokf run git status must not be rewritten — the skip guard
    // should fire on the env-stripped portion ("tokf run git status").
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "DEBUG=1 tokf run git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "DEBUG=1 tokf run git status");
}

#[test]
fn rewrite_user_skip_pattern_matches_env_stripped_command() {
    // A user skip pattern like "^git" does NOT match "FOO=bar git status"
    // because skip patterns operate on the full segment (env prefix included).
    // Users who want to skip regardless of prefix must account for it in the pattern.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: Some(types::SkipConfig {
            patterns: vec!["^git".to_string()],
        }),
        rewrite: vec![],
    };
    // "FOO=bar git status" does NOT start with "git", so skip does not fire
    // and the command IS rewritten.
    let r = rewrite_with_config(
        "FOO=bar git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "FOO=bar tokf run git status");
}
