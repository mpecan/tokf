#![allow(clippy::unwrap_used)]

use std::process::Command;

fn tokf() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    // Use a unique temp DB per invocation to avoid cross-test interference.
    let tmp = std::env::temp_dir().join(format!("tokf_generic_test_{}.db", std::process::id()));
    cmd.env("TOKF_DB_PATH", &tmp);
    // Disable the config cache so tests don't interfere.
    cmd.arg("--no-cache");
    cmd
}

#[test]
fn err_extracts_errors_from_echo() {
    let output = tokf()
        .args(["err", "echo", "error: something broke"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error: something broke"),
        "stdout: {stdout}"
    );
    // Short output with errors should still get the [tokf err] header
    assert!(stdout.contains("[tokf err]"), "stdout: {stdout}");
}

#[test]
fn test_extracts_failures_from_echo() {
    let output = tokf()
        .args(["test", "echo", "FAILED: test_foo"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("FAILED"), "stdout: {stdout}");
}

#[test]
fn summary_summarizes_echo() {
    let output = tokf()
        .args(["summary", "echo", "hello world"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello world"), "stdout: {stdout}");
}

#[test]
fn err_with_context_flag() {
    let output = tokf()
        .args(["err", "-C", "1", "echo", "error: oops"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("error: oops"), "stdout: {stdout}");
}

#[test]
fn summary_with_max_lines_flag() {
    let output = tokf()
        .args(["summary", "--max-lines", "10", "echo", "done"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("done"), "stdout: {stdout}");
}

#[test]
fn err_nonzero_exit_masked() {
    let output = tokf()
        .args(["err", "sh", "-c", "echo 'error: fail' && exit 1"])
        .output()
        .unwrap();
    // Exit code should be masked to 0 by default
    assert!(output.status.success(), "should exit 0 with masking");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Error: Exit code 1"), "stdout: {stdout}");
}

#[test]
fn err_nonzero_exit_no_mask() {
    let output = tokf()
        .args([
            "--no-mask-exit-code",
            "err",
            "sh",
            "-c",
            "echo 'error: fail' && exit 1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "should propagate exit code");
}
