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
fn info_tokf_home_shown_in_output() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .arg("info")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("TOKF_HOME:"),
        "expected TOKF_HOME line in output:\n{stdout}"
    );
    assert!(
        stdout.contains(tmp.path().to_str().unwrap()),
        "expected TOKF_HOME value in output:\n{stdout}"
    );
}

#[test]
fn info_tokf_home_redirects_user_filter_dir() {
    let tmp = TempDir::new().unwrap();
    let home_dir = tmp.path().join("myhome");
    std::fs::create_dir_all(home_dir.join("filters")).unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home_dir.to_str().unwrap())
        .arg("info")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // The [user] search dir should appear under TOKF_HOME
    assert!(
        stdout.contains(home_dir.to_str().unwrap()),
        "expected TOKF_HOME path in filter search dirs:\n{stdout}"
    );
}

#[test]
fn info_json_includes_home_override() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");
    // home_override is now a top-level field, not nested under tracking_db
    assert!(
        parsed["home_override"].is_string(),
        "home_override should be a top-level string when TOKF_HOME is set, got: {parsed}"
    );
    assert_eq!(
        parsed["home_override"].as_str().unwrap(),
        tmp.path().to_str().unwrap()
    );
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

#[test]
fn info_json_includes_access_fields() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .args(["info", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).unwrap();

    // cache and tracking_db should carry an `access` field
    assert!(
        parsed["cache"]["access"].is_string(),
        "cache.access should be a string, got: {}",
        parsed["cache"]
    );
    assert!(
        parsed["tracking_db"]["access"].is_string(),
        "tracking_db.access should be a string, got: {}",
        parsed["tracking_db"]
    );

    // search dirs should carry an `access` field (null when not found, string when found)
    let dirs = parsed["search_dirs"].as_array().unwrap();
    let local = &dirs[0];
    // `access` is null when the dir does not exist, a string label when it does
    assert!(
        local["access"].is_null() || local["access"].is_string(),
        "search_dirs[0].access should be null or string, got: {}",
        local["access"]
    );
}

#[test]
fn info_human_shows_access_status() {
    let tmp = TempDir::new().unwrap();
    let output = tokf().current_dir(tmp.path()).arg("info").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // The cache and DB lines should contain a parenthesised access label.
    // In a clean temp directory the DB does not exist yet; the cache may or may not.
    // Either way we expect one of the known labels to appear.
    let known_labels = [
        "writable",
        "read-only!",
        "will be created",
        "dir not writable!",
    ];
    assert!(
        known_labels.iter().any(|l| stdout.contains(l)),
        "expected an access label in output:\n{stdout}"
    );
}
