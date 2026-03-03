#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

/// Returns a Command with `TOKF_HOME` set to a temp dir so the subprocess
/// never probes the real OS keychain.
fn tokf_isolated() -> (Command, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    cmd.env("TOKF_HOME", dir.path());
    (cmd, dir)
}

#[test]
fn auth_status_not_logged_in() {
    let (mut cmd, _dir) = tokf_isolated();
    let output = cmd.args(["auth", "status"]).output().unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Not logged in"),
        "expected 'Not logged in' with empty TOKF_HOME, got: {stdout}"
    );
}

#[test]
fn auth_logout_when_not_logged_in() {
    let (mut cmd, _dir) = tokf_isolated();
    let output = cmd.args(["auth", "logout"]).output().unwrap();
    assert!(
        output.status.success(),
        "expected exit 0 (idempotent logout), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nothing to do"),
        "expected 'nothing to do' with empty TOKF_HOME, got: {stderr}"
    );
}

#[test]
fn auth_login_unreachable_server() {
    let (mut cmd, _dir) = tokf_isolated();
    let output = cmd
        .env("TOKF_SERVER_URL", "http://localhost:1")
        .args(["auth", "login"])
        .output()
        .unwrap();
    // Should fail gracefully, not panic
    assert!(
        !output.status.success(),
        "expected non-zero exit for unreachable server"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] error:"),
        "expected error message, got: {stderr}"
    );
}

#[test]
fn auth_help_shows_subcommands() {
    let (mut cmd, _dir) = tokf_isolated();
    let output = cmd.args(["auth", "--help"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("login"),
        "expected 'login' in help: {stdout}"
    );
    assert!(
        stdout.contains("logout"),
        "expected 'logout' in help: {stdout}"
    );
    assert!(
        stdout.contains("status"),
        "expected 'status' in help: {stdout}"
    );
}
