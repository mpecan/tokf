//! Tests for the `[pipe]` config section: strip toggle and prefer-less flag injection.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

use super::*;

// --- pipe strip toggle ---

#[test]
fn rewrite_pipe_strip_disabled_preserves_pipe() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: false,
            prefer_less: false,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "cargo test | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // strip=false: pipe is preserved, command passes through unchanged.
    assert_eq!(r, "cargo test | tail -5");
}

#[test]
fn rewrite_pipe_strip_disabled_non_piped_still_rewritten() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: false,
            prefer_less: false,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "cargo test --lib",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // Non-piped commands are still rewritten normally.
    assert_eq!(r, "tokf run cargo test --lib");
}

// --- prefer_less flag injection ---

#[test]
fn rewrite_prefer_less_injects_flag() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: true,
            prefer_less: true,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "cargo test | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run --baseline-pipe 'tail -5' --prefer-less cargo test"
    );
}

#[test]
fn rewrite_prefer_less_without_pipe_no_effect() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: true,
            prefer_less: true,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "cargo test --lib",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // No pipe to strip, so --prefer-less is not injected.
    assert_eq!(r, "tokf run cargo test --lib");
}

#[test]
fn rewrite_strip_false_overrides_prefer_less() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: false,
            prefer_less: true,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "cargo test | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // strip=false takes priority — pipe is preserved, prefer_less has no effect.
    assert_eq!(r, "cargo test | tail -5");
}

#[test]
fn rewrite_compound_prefer_less_per_segment() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

    let config = RewriteConfig {
        skip: None,
        pipe: Some(types::PipeConfig {
            strip: true,
            prefer_less: true,
        }),
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "git add . && git diff | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run git add . && tokf run --baseline-pipe 'head -5' --prefer-less git diff"
    );
}

// --- inject_pipe_flags direct unit tests ---

#[test]
fn inject_pipe_flags_normal() {
    let r = inject_pipe_flags("tokf run cargo test", "tail -5", false);
    assert_eq!(r, "tokf run --baseline-pipe 'tail -5' cargo test");
}

#[test]
fn inject_pipe_flags_with_prefer_less() {
    let r = inject_pipe_flags("tokf run cargo test", "tail -5", true);
    assert_eq!(
        r,
        "tokf run --baseline-pipe 'tail -5' --prefer-less cargo test"
    );
}

#[test]
fn inject_pipe_flags_non_tokf_prefix_passthrough() {
    let r = inject_pipe_flags("some-other-wrapper cargo test", "tail -5", false);
    assert_eq!(r, "some-other-wrapper cargo test");
}

#[test]
fn inject_pipe_flags_single_quote_escaping() {
    let r = inject_pipe_flags("tokf run cargo test", "grep -E 'fail'", false);
    assert_eq!(
        r,
        "tokf run --baseline-pipe 'grep -E '\\''fail'\\''' cargo test"
    );
}

#[test]
fn inject_pipe_flags_empty_suffix() {
    let r = inject_pipe_flags("tokf run cargo test", "", false);
    assert_eq!(r, "tokf run --baseline-pipe '' cargo test");
}
