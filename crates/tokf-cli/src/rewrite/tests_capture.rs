//! Tests for `[pipe] capture` — pipeline capture planning.
//!
//! The declines are asserted first and in bulk. A capture implementation
//! validated only on what it permits looks finished while doing nothing; the
//! prototype this feature came from passed every "leave alone" case and failed
//! exactly the two that mattered.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;

/// A filter dir containing a `cargo test` filter, so tests can tell "declined
/// by capture" apart from "no filter existed anyway".
fn filter_dir() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();
    dir
}

fn config(capture: bool) -> RewriteConfig {
    RewriteConfig {
        pipe: Some(types::PipeConfig {
            capture,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn rewrite_with(cmd: &str, cfg: &RewriteConfig, dir: &TempDir) -> String {
    let dirs = vec![PathBuf::from(dir.path())];
    rewrite_isolated(cmd, cfg, &dirs, false)
}

/// Capture on, and the command is expected to be wrapped.
fn captured(cmd: &str) -> String {
    let dir = filter_dir();
    rewrite_with(cmd, &config(true), &dir)
}

/// Capture on, but the command is expected to come back untouched.
fn assert_declined(cmd: &str) {
    let dir = filter_dir();
    let got = rewrite_with(cmd, &config(true), &dir);
    assert_eq!(got, cmd, "expected `{cmd}` to be declined by capture");
}

// --- declines ---

#[test]
fn declines_when_capture_is_off() {
    let dir = filter_dir();
    let cmd = "cargo test | tail -10 | grep ERROR";
    assert_eq!(rewrite_with(cmd, &config(false), &dir), cmd);
}

#[test]
fn declines_when_author_handles_pipe_status() {
    // The wrapper rewrite may still fire here — that is pre-existing and
    // orthogonal. What must not happen is capture claiming the pipeline.
    for cmd in [
        "set -o pipefail; just check 2>&1 | tail -8",
        "just check 2>&1 | tail -8; exit ${PIPESTATUS[0]}",
        "set -o pipefail; cargo test | tail -10 | grep ERROR",
    ] {
        let dir = filter_dir();
        let got = rewrite_with(cmd, &config(true), &dir);
        assert!(
            !got.contains("--pipe-through"),
            "`{cmd}` should not be captured, got: {got}"
        );
    }
}

#[test]
fn declines_unbounded_producers() {
    assert_declined("tail -f app.log | grep ERROR");
    assert_declined("journalctl --follow | grep oom");
    assert_declined("docker logs -f web | grep ERROR");
    assert_declined("yes | head -3");
}

#[test]
fn declines_interactive_consumers() {
    assert_declined("cargo test | less");
    assert_declined("cargo test | tail -20 | fzf");
}

#[test]
fn declines_head_redirects_other_than_stderr_merge() {
    // Suppressed stderr must stay suppressed — tokf forwards the head's stderr
    // to its own, which would resurrect exactly what was silenced.
    assert_declined("cargo test 2>/dev/null | grep x");
}

#[test]
fn declines_when_user_denylist_names_the_consumer() {
    let dir = filter_dir();
    let mut cfg = config(true);
    cfg.pipe.as_mut().unwrap().capture_deny = vec!["mytool".to_string()];
    let cmd = "cargo test | mytool";
    assert_eq!(rewrite_with(cmd, &cfg, &dir), cmd);
}

#[test]
fn existing_strip_path_keeps_priority() {
    // Single pipe to a strippable target with a matching filter: the
    // established `--baseline-pipe` behaviour must win over capture.
    let got = captured("cargo test | tail -5");
    assert!(
        got.contains("--baseline-pipe"),
        "strip path should win, got: {got}"
    );
    assert!(!got.contains("--pipe-through"), "got: {got}");
}

#[test]
fn declines_output_redirect_to_file() {
    assert_declined("cargo test | tail -5 > out.txt");
}

// --- captures ---

#[test]
fn captures_multi_pipe_chain() {
    let got = captured("cargo test | tail -10 | grep ERROR");
    assert_eq!(
        got,
        "tokf run --pipe-through 'tail -10 | grep ERROR' cargo test"
    );
}

#[test]
fn captures_unsupported_pipe_target() {
    let got = captured("kubectl get pods | wc -l");
    assert_eq!(got, "tokf run --pipe-through 'wc -l' kubectl get pods");
}

#[test]
fn captures_when_base_has_no_filter() {
    let got = captured("curl -s https://example.com | jq .name");
    assert_eq!(
        got,
        "tokf run --pipe-through 'jq .name' curl -s https://example.com"
    );
}

#[test]
fn captures_through_the_just_wrapper_hole() {
    // The `just` wrapper pattern ends in `(\s.*)?$`, which swallows
    // ` check | tail -5` whole. Before capture this returned the pipeline
    // untouched — and the swallowed exit code with it.
    let got = captured("just check | tail -5");
    assert!(
        got.starts_with("tokf run --pipe-through 'tail -5' "),
        "{got}"
    );
    // The wrapper still applies, nested inside the capture, so recipe-line
    // filtering is not lost.
    assert!(got.contains("just --shell tokf"), "{got}");
}

#[test]
fn captures_the_original_defect() {
    // `2>&1` moves off the head and becomes `--merge-stderr`. Left in place it
    // would be applied by the shell to the `tokf run` process itself — merging
    // tokf's own notes into stdout while still feeding the pipeline stdout
    // alone, which is the opposite of what the caller wrote.
    let got = captured("timeout 900 just check 2>&1 | tail -8");
    assert_eq!(
        got,
        "tokf run --pipe-through 'tail -8' --merge-stderr timeout 900 just check"
    );
}

#[test]
fn no_merge_flag_without_a_stderr_redirect() {
    let got = captured("cargo test | tail -10 | grep ERROR");
    assert!(!got.contains("--merge-stderr"), "{got}");
}

#[test]
fn propagate_mode_emits_its_flag() {
    let dir = filter_dir();
    let mut cfg = config(true);
    cfg.pipe.as_mut().unwrap().capture_exit = types::CaptureExit::Propagate;
    let got = rewrite_with("cargo test | wc -l", &cfg, &dir);
    assert!(got.contains("--propagate-exit"), "{got}");
}

#[test]
fn captures_preserves_env_prefix() {
    let got = captured("RUST_LOG=debug cargo test | wc -l");
    assert_eq!(
        got,
        "RUST_LOG=debug tokf run --pipe-through 'wc -l' cargo test"
    );
}

#[test]
fn captures_escapes_single_quotes_in_the_tail() {
    let got = captured("cargo test | grep -E 'fail|error' | wc -l");
    assert_eq!(
        got,
        r"tokf run --pipe-through 'grep -E '\''fail|error'\'' | wc -l' cargo test"
    );
}

#[test]
fn captures_status_consuming_pipeline_because_exit_code_is_untouched() {
    // In the default "report" mode nothing downstream changes, so a pipeline
    // whose status is consumed on purpose is safe to capture.
    let dir = filter_dir();
    let got = rewrite_with(
        "cargo build | grep -q warning && echo found",
        &config(true),
        &dir,
    );
    assert!(
        got.starts_with("tokf run --pipe-through 'grep -q warning'"),
        "{got}"
    );
    assert!(got.ends_with("&& echo found"), "{got}");
}

#[test]
fn propagate_mode_declines_status_consuming_pipelines() {
    let dir = filter_dir();
    let mut cfg = config(true);
    cfg.pipe.as_mut().unwrap().capture_exit = types::CaptureExit::Propagate;

    let cmd = "cargo build | grep -q warning && echo found";
    assert_eq!(rewrite_with(cmd, &cfg, &dir), cmd);

    let cmd = "cargo test | wc -l && echo done";
    assert_eq!(rewrite_with(cmd, &cfg, &dir), cmd);

    // Without a status-consuming chain, propagate mode captures normally.
    let got = rewrite_with("cargo test | wc -l", &cfg, &dir);
    assert!(got.contains("--pipe-through"), "{got}");
}

#[test]
fn capture_is_idempotent() {
    let dir = filter_dir();
    let once = rewrite_with("cargo test | tail -10 | grep ERROR", &config(true), &dir);
    let twice = rewrite_with(&once, &config(true), &dir);
    assert_eq!(once, twice);
}

// --- 2>&1 stream selection ---

#[test]
fn stderr_merge_is_detected_from_the_ast_not_a_regex() {
    // A regex splitter cutting `2>&1` at the `&` parses this as two statements
    // and sails through — the exact command this feature exists to catch.
    let parsed = bash_ast::ParsedCommand::parse("timeout 900 just check 2>&1 | tail -8").unwrap();
    let cfg = types::PipeConfig {
        capture: true,
        ..Default::default()
    };
    let cap = capture::plan(&parsed, &cfg).expect("should plan a capture");
    assert_eq!(cap.head, "timeout 900 just check");
    assert_eq!(cap.tail, "tail -8");
    assert!(cap.merge_stderr, "2>&1 must select the combined stream");
}

#[test]
fn no_redirect_means_stdout_only() {
    let parsed = bash_ast::ParsedCommand::parse("cargo test | grep error").unwrap();
    let cfg = types::PipeConfig {
        capture: true,
        ..Default::default()
    };
    let cap = capture::plan(&parsed, &cfg).unwrap();
    assert!(
        !cap.merge_stderr,
        "without 2>&1 the pipeline must see stdout only"
    );
}

// --- stage names come from the AST, not from splitting on '|' ---

#[test]
fn a_quoted_pipe_is_not_a_stage_boundary() {
    let parsed = bash_ast::ParsedCommand::parse("cargo test | grep -E 'a|b' | wc -l").unwrap();
    // Two pipes, three stages — the `|` inside the quoted regex is not one.
    assert_eq!(
        parsed.pipeline_stages(),
        vec!["cargo test", "grep -E 'a|b'", "wc -l"]
    );
}

#[test]
fn the_denylist_sees_whole_stage_names_not_fragments() {
    let dir = filter_dir();
    let mut cfg = config(true);
    cfg.pipe.as_mut().unwrap().capture_deny = vec!["less".to_string()];
    // `less` appears only inside a quoted pattern, so it is not a stage.
    let got = rewrite_with("cargo test | grep -E 'more|less' | wc -l", &cfg, &dir);
    assert!(got.contains("--pipe-through"), "{got}");
    // But as an actual stage it must still be declined.
    let cmd = "cargo test | wc -l | less";
    assert_eq!(rewrite_with(cmd, &cfg, &dir), cmd);
}

#[test]
fn propagate_guard_fires_when_the_pattern_contains_a_pipe() {
    // Regression: `rsplit('|')` saw the last stage as `b'`, so the `grep -q`
    // guard did not fire and propagate mode inverted a predicate pipeline.
    let dir = filter_dir();
    let mut cfg = config(true);
    cfg.pipe.as_mut().unwrap().capture_exit = types::CaptureExit::Propagate;
    for cmd in [
        "cargo build | grep -q warning",
        "cargo build | grep -q 'a|b'",
        "cargo build | wc -l | grep -q 'x|y'",
    ] {
        assert_eq!(
            rewrite_with(cmd, &cfg, &dir),
            cmd,
            "propagate mode must decline the predicate pipeline `{cmd}`"
        );
    }
}

// --- 2>&1 excision is byte-preserving ---

#[test]
fn stripping_the_stderr_merge_does_not_rewrite_the_rest_of_the_head() {
    // Regression: re-tokenising on whitespace collapsed the double space and
    // redistributed the quotes, silently changing the commit message.
    let got = captured(r#"git commit -m "a  b" 2>&1 | wc -l"#);
    assert_eq!(
        got,
        r#"tokf run --pipe-through 'wc -l' --merge-stderr git commit -m "a  b""#
    );
}

#[test]
fn a_non_trailing_stderr_merge_is_declined_rather_than_guessed_at() {
    assert_declined("sh -c 2>&1 'echo hi' | wc -l");
}
