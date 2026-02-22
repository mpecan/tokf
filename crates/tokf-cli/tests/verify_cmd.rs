#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::process::Command;

fn tokf() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

// --- verify cargo/build ---

#[test]
fn verify_cargo_build_passes() {
    let output = tokf().args(["verify", "cargo/build"]).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo/build"),
        "expected suite name in output"
    );
}

// --- verify all stdlib suites ---

#[test]
fn verify_all_stdlib_suites_pass() {
    let output = tokf().args(["verify"]).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "Some suites failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show a summary
    assert!(
        stdout.contains("passed"),
        "expected 'passed' summary in output"
    );
}

// --- --list flag ---

#[test]
fn verify_list_shows_suites() {
    let output = tokf().args(["verify", "--list"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo/build"),
        "expected 'cargo/build' in list output, got:\n{stdout}"
    );
    assert!(
        stdout.contains("git/push"),
        "expected 'git/push' in list output, got:\n{stdout}"
    );
    // --list should show case counts
    assert!(
        stdout.contains("case"),
        "expected case counts in list output, got:\n{stdout}"
    );
}

// --- --json flag ---

#[test]
fn verify_json_output_is_valid() {
    let output = tokf()
        .args(["verify", "cargo/build", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");
    let arr = parsed.as_array().expect("JSON root should be an array");
    assert!(!arr.is_empty(), "JSON array should not be empty");
    let suite = &arr[0];
    assert!(
        suite.get("filter_name").is_some(),
        "suite should have filter_name"
    );
    assert!(suite.get("cases").is_some(), "suite should have cases");
}

// --- missing filter exits 2 ---

#[test]
fn verify_missing_filter_exits_2() {
    let output = tokf().args(["verify", "no/such/filter"]).output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no/such/filter"),
        "expected filter name in error message, got:\n{stderr}"
    );
}

// --- failing expectation exits 1 ---

#[test]
fn verify_failing_expectation_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let filters_dir = dir.path().join("filters").join("mytest");
    fs::create_dir_all(&filters_dir).unwrap();
    let suite_dir = dir.path().join("filters").join("mytest").join("cmd_test");
    fs::create_dir_all(&suite_dir).unwrap();

    // Write a minimal filter TOML
    let filter_toml = filters_dir.join("cmd.toml");
    fs::write(
        &filter_toml,
        r#"command = "mytest cmd"

[on_success]
output = "filtered output"
"#,
    )
    .unwrap();

    // Write a test case with a failing assertion
    let case_toml = suite_dir.join("bad.toml");
    fs::write(
        &case_toml,
        r#"name = "intentionally failing case"
inline = "hello world"
exit_code = 0

[[expect]]
equals = "this will never match"
"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_tokf"))
        .args(["verify", "mytest/cmd"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit 1 for failing assertion\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("this will never match") || stdout.contains("intentionally failing"),
        "expected failure detail in output, got:\n{stdout}"
    );
}
