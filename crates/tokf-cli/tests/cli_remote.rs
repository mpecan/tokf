#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// `remote status` must exit 0 regardless of registration state.
/// We isolate the test by pointing HOME at a temp directory so the result
/// is deterministic: always "Not registered".
#[test]
fn remote_status_exits_zero() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .args(["remote", "status"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Not registered"),
        "expected 'Not registered' with clean HOME, got: {stdout}"
    );
}

/// `remote setup` must fail when no credentials exist in the isolated HOME.
#[test]
fn remote_setup_requires_login() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .env("TOKF_SERVER_URL", "http://localhost:1")
        .args(["remote", "setup"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit when not logged in"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] error:"),
        "expected error message, got: {stderr}"
    );
}

#[test]
fn remote_help_shows_subcommands() {
    let output = tokf().args(["remote", "--help"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("setup"),
        "expected 'setup' in help: {stdout}"
    );
    assert!(
        stdout.contains("status"),
        "expected 'status' in help: {stdout}"
    );
}
