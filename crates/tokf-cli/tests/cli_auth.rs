#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

#[test]
fn auth_status_not_logged_in() {
    let output = tokf().args(["auth", "status"]).output().unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // When no credentials are stored, should report not logged in.
    // If the developer has logged in locally, the test still passes but checks
    // the alternative output.
    assert!(
        stdout.contains("Not logged in") || stdout.contains("Logged in as"),
        "expected auth status output, got: {stdout}"
    );
}

#[test]
fn auth_logout_when_not_logged_in() {
    let output = tokf().args(["auth", "logout"]).output().unwrap();
    assert!(
        output.status.success(),
        "expected exit 0 (idempotent logout), stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should say either "Logged out" or "Not logged in, nothing to do."
    assert!(
        stderr.contains("Logged out") || stderr.contains("nothing to do"),
        "expected logout message, got: {stderr}"
    );
}

#[test]
fn auth_login_unreachable_server() {
    let output = tokf()
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
    let output = tokf().args(["auth", "--help"]).output().unwrap();
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
