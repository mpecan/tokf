#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for local environment wrapper unwrapping in the rewrite path — see
//! issue #403.
//!
//! `nix develop -c cargo test` runs `cargo test` locally, so tokf unwraps the
//! prefix to match the `cargo test` filter and wraps the *whole* command with
//! `tokf run` (outer wrap — tokf is the parent and filters the combined
//! output). Contrast with transparent-arg commands (ssh), which are remote.

use std::fs;

use tempfile::TempDir;

use super::*;
use types::{LocalWrapperConfig, LocalWrapperRule};

/// A tempdir search dir holding a `cargo test` filter.
fn cargo_test_filters() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();
    dir
}

#[test]
fn wraps_whole_command_outer() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop -c cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run nix develop -c cargo test");
}

#[test]
fn wraps_with_attr_and_flags() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop .#agent --impure -c cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        result,
        "tokf run nix develop .#agent --impure -c cargo test"
    );
}

#[test]
fn wraps_long_form_command_marker() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop --command cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run nix develop --command cargo test");
}

#[test]
fn pipe_composition_wraps_and_strips() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop -c cargo test | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        result,
        "tokf run --baseline-pipe 'tail -5' nix develop -c cargo test"
    );
}

#[test]
fn env_prefix_composition() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "FOO=bar nix develop -c cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "FOO=bar tokf run nix develop -c cargo test");
}

#[test]
fn compound_composition_wraps_each_segment() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();
    fs::write(
        dir.path().join("cargo-build.toml"),
        "command = \"cargo build\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop -c cargo test && nix develop -c cargo build",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        result,
        "tokf run nix develop -c cargo test && tokf run nix develop -c cargo build"
    );
}

#[test]
fn unmatched_inner_passes_through() {
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop -c echo hi",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "nix develop -c echo hi");
}

#[test]
fn make_nesting_passes_through_documented_limitation() {
    // `make` is handled by SHELL=tokf injection, which would need tokf inside
    // the devshell — so a make command nested in a local wrapper is NOT
    // rewritten. It passes through unchanged (only the outer level could be
    // filtered, and make has no filter of its own).
    let dir = cargo_test_filters();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "nix develop -c make check",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "nix develop -c make check");
}

#[test]
fn disabled_builtin_leaves_command_unrewritten() {
    let dir = cargo_test_filters();
    let config = RewriteConfig {
        local_wrapper: Some(LocalWrapperConfig {
            builtins: true,
            disabled: vec!["nix".to_string()],
            rules: vec![],
        }),
        ..RewriteConfig::default()
    };
    let result = rewrite_with_config(
        "nix develop -c cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "nix develop -c cargo test");
}

#[test]
fn builtins_off_leaves_command_unrewritten() {
    let dir = cargo_test_filters();
    let config = RewriteConfig {
        local_wrapper: Some(LocalWrapperConfig {
            builtins: false,
            disabled: vec![],
            rules: vec![],
        }),
        ..RewriteConfig::default()
    };
    let result = rewrite_with_config(
        "nix develop -c cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "nix develop -c cargo test");
}

#[test]
fn user_rule_wraps_custom_wrapper() {
    let dir = cargo_test_filters();
    let config = RewriteConfig {
        local_wrapper: Some(LocalWrapperConfig {
            builtins: true,
            disabled: vec![],
            rules: vec![LocalWrapperRule {
                command: "distrobox".to_string(),
                subcommands: vec!["enter".to_string()],
                markers: vec!["--".to_string()],
            }],
        }),
        ..RewriteConfig::default()
    };
    let result = rewrite_with_config(
        "distrobox enter my-box -- cargo test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run distrobox enter my-box -- cargo test");
}
