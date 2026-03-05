#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// `tokf search` with no query arguments must fail (required positional).
#[test]
fn search_requires_at_least_one_word() {
    let output = tokf().args(["search"]).output().unwrap();
    assert!(
        !output.status.success(),
        "expected non-zero exit for missing query"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required") || stderr.contains("Usage"),
        "expected usage/required message, got: {stderr}"
    );
}

/// `tokf search --help` must mention the query positional and flags.
#[test]
fn search_help_shows_query_and_flags() {
    let output = tokf().args(["search", "--help"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("query"),
        "expected 'query' in help: {stdout}"
    );
    assert!(
        stdout.contains("--json"),
        "expected '--json' in help: {stdout}"
    );
    assert!(
        stdout.contains("--limit") || stdout.contains("-n"),
        "expected '--limit' or '-n' in help: {stdout}"
    );
}

/// Multi-word query without quotes is accepted by the CLI argument parser.
/// The actual search will fail (no auth), but we verify the args are parsed
/// without a clap error — the error should come from the search logic, not
/// from argument parsing.
#[test]
fn search_accepts_multi_word_query() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .args(["search", "git", "push"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should fail with an auth/network error, NOT a clap argument parsing error.
    assert!(
        !stderr.contains("Usage") && !stderr.contains("required"),
        "multi-word query should be accepted by arg parser, got: {stderr}"
    );
}

/// Flags before positional args must be parsed correctly.
#[test]
fn search_flags_before_query() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .args(["search", "--json", "-n", "5", "git", "push"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Usage") && !stderr.contains("required"),
        "flags before query should be accepted, got: {stderr}"
    );
}
