#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// Create a temp dir with a project-scoped filter and test suite.
fn setup_project_filter(tmp: &TempDir) {
    let filter_dir = tmp.path().join(".tokf/filters/test");
    fs::create_dir_all(&filter_dir).unwrap();
    fs::write(filter_dir.join("hello.toml"), "command = \"test-hello\"\n").unwrap();

    let suite_dir = filter_dir.join("hello_test");
    fs::create_dir_all(&suite_dir).unwrap();
    fs::write(
        suite_dir.join("basic.toml"),
        r#"name = "basic"
inline = "hello world"

[[expect]]
contains = "hello"
"#,
    )
    .unwrap();
}

/// Create a temp dir with a stdlib-style filter and test suite (filters/ in CWD).
fn setup_stdlib_filter(tmp: &TempDir) {
    let filter_dir = tmp.path().join("filters/test");
    fs::create_dir_all(&filter_dir).unwrap();
    fs::write(filter_dir.join("world.toml"), "command = \"test-world\"\n").unwrap();

    let suite_dir = filter_dir.join("world_test");
    fs::create_dir_all(&suite_dir).unwrap();
    fs::write(
        suite_dir.join("basic.toml"),
        r#"name = "basic"
inline = "hello world"

[[expect]]
contains = "world"
"#,
    )
    .unwrap();
}

#[test]
fn verify_scope_project_finds_project_filters() {
    let tmp = TempDir::new().unwrap();
    setup_project_filter(&tmp);

    let output = tokf()
        .current_dir(tmp.path())
        .args(["verify", "--scope", "project", "--list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("test/hello"),
        "expected project filter in list:\n{stdout}"
    );
}

#[test]
fn verify_scope_project_ignores_stdlib() {
    let tmp = TempDir::new().unwrap();
    // Only create stdlib-style filters, no project filters
    setup_stdlib_filter(&tmp);

    let output = tokf()
        .current_dir(tmp.path())
        .args(["verify", "--scope", "project", "--list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should not find the stdlib filter
    assert!(
        !stdout.contains("test/world"),
        "project scope should not include stdlib filters:\n{stdout}"
    );
    // Project scope with no .tokf/filters/ → empty discovery → stderr message
    assert!(
        stdout.is_empty(),
        "expected empty stdout for empty scope:\n{stdout}"
    );
    assert!(
        stderr.contains("no test suites discovered"),
        "expected 'no test suites discovered' on stderr:\n{stderr}"
    );
}

// Note: --scope global is not integration-tested because the global config dir
// (e.g. ~/.config/tokf/filters/) is OS-specific and shared across the user's
// session. The unit tests in verify_cmd.rs::tests cover the path logic.

#[test]
fn verify_scope_stdlib_finds_stdlib_filters() {
    let tmp = TempDir::new().unwrap();
    setup_stdlib_filter(&tmp);

    let output = tokf()
        .current_dir(tmp.path())
        .args(["verify", "--scope", "stdlib", "--list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("test/world"),
        "expected stdlib filter in list:\n{stdout}"
    );
}

#[test]
fn verify_no_scope_unchanged() {
    // Default behavior (no --scope) should still work.
    let tmp = TempDir::new().unwrap();
    setup_project_filter(&tmp);
    setup_stdlib_filter(&tmp);

    let output = tokf()
        .current_dir(tmp.path())
        .args(["verify", "--list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Should find both
    assert!(
        stdout.contains("test/hello"),
        "expected project filter:\n{stdout}"
    );
    assert!(
        stdout.contains("test/world"),
        "expected stdlib filter:\n{stdout}"
    );
}
