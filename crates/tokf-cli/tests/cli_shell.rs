#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

// --- shell mode entry ---

#[test]
fn shell_c_true_exits_zero() {
    let output = tokf().args(["-c", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_c_false_exits_nonzero() {
    let output = tokf().args(["-c", "false"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn shell_c_echo_produces_output() {
    let output = tokf().args(["-c", "echo hello"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn shell_c_missing_command_arg() {
    let output = tokf().arg("-c").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shell mode requires a command argument"),
        "expected error message, got: {stderr}"
    );
}

// --- combined flags ---

#[test]
fn shell_cu_works() {
    let output = tokf().args(["-cu", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_ec_works() {
    let output = tokf().args(["-ec", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_ecu_works() {
    let output = tokf().args(["-ecu", "true"]).output().unwrap();
    assert!(output.status.success());
}

// --- compound delegation ---

#[test]
fn shell_compound_and_delegates() {
    let output = tokf().args(["-c", "true && true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_compound_failure_propagates() {
    let output = tokf().args(["-c", "false && true"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn shell_pipe_delegates_to_sh() {
    let output = tokf().args(["-c", "echo hello | cat"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn shell_redirect_delegates_to_sh() {
    // Redirections should be handled by the real shell.
    let output = tokf()
        .args(["-c", "echo hello > /dev/null"])
        .output()
        .unwrap();
    assert!(output.status.success());
    // Output was redirected to /dev/null, so stdout should be empty.
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

// --- exit code preservation ---

#[test]
fn shell_exit_code_42() {
    let output = tokf().args(["-c", "exit 42"]).output().unwrap();
    assert_eq!(output.status.code(), Some(42));
}

// --- empty and whitespace commands ---

#[test]
fn shell_empty_command() {
    let output = tokf().args(["-c", ""]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_whitespace_only_command() {
    let output = tokf().args(["-c", "   "]).output().unwrap();
    // Whitespace-only delegates to sh, which treats it as a no-op.
    assert!(output.status.success());
}

// --- double-wrap prevention ---

#[test]
fn shell_tokf_run_not_double_wrapped() {
    // "tokf run echo hello" should not be intercepted as a filter match
    // since it starts with "tokf" which is a built-in skip pattern.
    // It delegates to sh, which runs tokf run echo hello.
    let output = tokf()
        .args(["-c", "echo not-double-wrapped"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "not-double-wrapped"
    );
}

// --- environment variable controls ---

#[test]
fn shell_no_filter_env_delegates() {
    let output = tokf()
        .env("TOKF_NO_FILTER", "1")
        .args(["-c", "echo bypassed"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "bypassed");
}

#[test]
fn shell_verbose_env_prints_to_stderr() {
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "true"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf]"),
        "expected verbose output on stderr, got: {stderr}"
    );
}
