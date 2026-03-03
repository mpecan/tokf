#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

// --- tokf check ---

#[test]
fn check_valid_filter() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let output = tokf().args(["check", &filter]).output().unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("valid"),
        "expected 'valid' in stderr, got: {stderr}"
    );
}

#[test]
fn check_nonexistent_file() {
    let output = tokf()
        .args(["check", "/nonexistent/path/filter.toml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr, got: {stderr}"
    );
}

#[test]
fn check_invalid_toml() {
    let dir = tempfile::TempDir::new().unwrap();
    let bad_toml = dir.path().join("bad.toml");
    std::fs::write(&bad_toml, "not valid toml [[[").unwrap();

    let output = tokf()
        .args(["check", bad_toml.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error"),
        "expected 'error' in stderr, got: {stderr}"
    );
}

// --- tokf test ---

#[test]
fn test_nonexistent_filter_exits_with_error() {
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());
    let output = tokf()
        .args(["test", "/nonexistent/filter.toml", &fixture])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("filter not found"),
        "expected 'filter not found' in stderr, got: {stderr}"
    );
}

#[test]
fn test_nonexistent_fixture_exits_with_error() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let output = tokf()
        .args(["test", &filter, "/nonexistent/fixture.txt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read fixture"),
        "expected fixture error in stderr, got: {stderr}"
    );
}

#[test]
fn test_exit_code_selects_different_branch() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());

    let success_output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "0"])
        .output()
        .unwrap();
    let failure_output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "1"])
        .output()
        .unwrap();

    let success_stdout = String::from_utf8_lossy(&success_output.stdout);
    let failure_stdout = String::from_utf8_lossy(&failure_output.stdout);

    assert_ne!(
        success_stdout.trim(),
        failure_stdout.trim(),
        "exit code should select different branches: success={success_stdout:?}, failure={failure_stdout:?}"
    );
}

#[test]
fn test_git_push_success_fixture() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok") && stdout.contains("main"),
        "expected filtered push output, got: {stdout}"
    );
}

#[test]
fn test_git_push_up_to_date_fixture() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/up_to_date.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok (up-to-date)");
}

#[test]
fn test_git_push_failure_with_exit_code() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/failure.txt", manifest_dir());
    let output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected failure branch output");
}

#[test]
fn test_with_timing() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/up_to_date.txt", manifest_dir());
    let output = tokf()
        .args(["test", "--timing", &filter, &fixture])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] filter took"),
        "expected timing info on stderr, got: {stderr}"
    );
}
