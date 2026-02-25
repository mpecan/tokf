#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// `tokf publish git/status` must fail: git/status is a built-in stdlib filter.
/// Users must eject it first before publishing.
#[test]
fn publish_builtin_filter_rejected() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .args(["publish", "git/status"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit for built-in filter, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built-in"),
        "expected 'built-in' in error message, got: {stderr}"
    );
}

/// `tokf publish --dry-run` with a local filter should preview and exit 0,
/// with no network calls.
#[test]
fn publish_dry_run_no_network() {
    let home = tempfile::tempdir().unwrap();

    // Create a project-local filter in `.tokf/filters/myns/test-filter.toml`
    let filter_dir = home.path().join(".tokf").join("filters").join("myns");
    std::fs::create_dir_all(&filter_dir).unwrap();
    std::fs::write(
        filter_dir.join("test-filter.toml"),
        r#"command = "my-test-command""#,
    )
    .unwrap();

    let output = tokf()
        .env("HOME", home.path())
        .current_dir(home.path())
        .args(["publish", "myns/test-filter", "--dry-run"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected exit 0 for dry-run, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("dry-run"),
        "expected 'dry-run' in output, got: {stderr}"
    );
    assert!(
        stderr.contains("my-test-command"),
        "expected command pattern in preview, got: {stderr}"
    );
}

/// `tokf publish --help` should exit 0 and show filter and dry-run flags.
#[test]
fn publish_help_shows_flags() {
    let output = tokf().args(["publish", "--help"]).output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("dry-run"),
        "expected '--dry-run' in help: {stdout}"
    );
    assert!(
        stdout.contains("filter"),
        "expected 'filter' in help: {stdout}"
    );
}

/// `tokf publish` with a nonexistent filter should fail with a "not found" error.
#[test]
fn publish_nonexistent_filter_fails() {
    let home = tempfile::tempdir().unwrap();
    let output = tokf()
        .env("HOME", home.path())
        .current_dir(home.path())
        .args(["publish", "definitely/does-not-exist-xyz"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "expected non-zero exit for missing filter"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found") || stderr.contains("[tokf] error"),
        "expected error message, got: {stderr}"
    );
}
