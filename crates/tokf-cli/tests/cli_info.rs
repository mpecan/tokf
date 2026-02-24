#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

#[test]
fn info_shows_version() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.starts_with("tokf "),
        "expected version line starting with 'tokf ', got:\n{stdout}"
    );
}

#[test]
fn info_shows_search_directories() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("filter search directories:"),
        "missing search dirs header:\n{stdout}"
    );
    assert!(
        stdout.contains("[built-in]"),
        "missing built-in label:\n{stdout}"
    );
    assert!(stdout.contains("[local]"), "missing local label:\n{stdout}");
}

#[test]
fn info_shows_tracking_db() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("tracking database:"),
        "missing tracking db header:\n{stdout}"
    );
    assert!(
        stdout.contains("TOKF_DB_PATH:"),
        "missing TOKF_DB_PATH line:\n{stdout}"
    );
}

#[test]
fn info_shows_cache_path() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("filter cache:"),
        "missing cache header:\n{stdout}"
    );
}

#[test]
fn info_shows_filter_counts() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("built-in:"),
        "missing built-in count:\n{stdout}"
    );
    assert!(stdout.contains("total:"), "missing total count:\n{stdout}");
}

#[test]
fn info_tokf_db_path_env_override() {
    let tmp = TempDir::new().unwrap();
    let custom_path = tmp.path().join("custom.db");

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_DB_PATH", custom_path.to_str().unwrap())
        .arg("info")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains(custom_path.to_str().unwrap()),
        "expected custom DB path in output:\n{stdout}"
    );
}

#[test]
fn info_json_is_valid() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("info --json should be valid JSON");
    assert!(parsed["version"].is_string());
    assert!(parsed["search_dirs"].is_array());
    assert!(parsed["tracking_db"].is_object());
    assert!(parsed["cache"].is_object());
    assert!(parsed["filters"].is_object());
    assert!(parsed["filters"]["builtin"].is_number());
    assert!(parsed["filters"]["total"].is_number());
}
