#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::items_after_statements
)]

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn tokf_with_db(db_path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    cmd.env("TOKF_DB_PATH", db_path);
    cmd
}

fn temp_db_dir() -> TempDir {
    TempDir::new().expect("tempdir")
}

#[test]
fn gain_empty_db_exits_zero() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    let status = tokf_with_db(&db)
        .args(["gain"])
        .status()
        .expect("run tokf gain");
    assert!(status.success(), "exit code: {:?}", status.code());
}

#[test]
fn gain_summary_shows_zeros_on_fresh_db() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    let out = tokf_with_db(&db)
        .args(["gain"])
        .output()
        .expect("run tokf gain");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("total runs:"), "stdout: {stdout}");
    assert!(stdout.contains('0'), "stdout: {stdout}");
}

#[test]
fn gain_records_run_after_tokf_run() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    // Run a command to generate a tracking row
    tokf_with_db(&db)
        .args(["run", "echo", "hello"])
        .output()
        .expect("run tokf run echo hello");
    // Now check gain
    let out = tokf_with_db(&db)
        .args(["gain"])
        .output()
        .expect("run tokf gain");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    // Should show at least 1 run
    assert!(stdout.contains("total runs:     1"), "stdout: {stdout}");
}

#[test]
fn gain_records_passthrough_when_no_filter_match() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    // Use a command that definitely has no filter
    tokf_with_db(&db)
        .args(["run", "echo", "passthrough_test"])
        .output()
        .expect("run");
    let out = tokf_with_db(&db)
        .args(["gain", "--by-filter", "--json"])
        .output()
        .expect("gain by-filter json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    // Passthrough rows show filter_name = "passthrough"
    assert!(
        stdout.contains("passthrough"),
        "expected 'passthrough' in: {stdout}"
    );
}

#[test]
fn gain_json_output_is_valid_json() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    let out = tokf_with_db(&db)
        .args(["gain", "--json"])
        .output()
        .expect("gain json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("must be valid JSON");
    // Must have the GainSummary fields
    assert!(parsed.get("total_commands").is_some(), "json: {parsed}");
    assert!(parsed.get("tokens_saved").is_some(), "json: {parsed}");
}

#[test]
fn gain_daily_json_is_array() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    tokf_with_db(&db)
        .args(["run", "echo", "hello"])
        .output()
        .expect("run");
    let out = tokf_with_db(&db)
        .args(["gain", "--daily", "--json"])
        .output()
        .expect("gain daily json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.is_array(), "expected array, got: {parsed}");
}

#[test]
fn gain_by_filter_shows_filter_name() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    tokf_with_db(&db)
        .args(["run", "echo", "hello"])
        .output()
        .expect("run");
    let out = tokf_with_db(&db)
        .args(["gain", "--by-filter"])
        .output()
        .expect("gain by-filter");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(
        stdout.contains("passthrough"),
        "expected filter row in: {stdout}"
    );
}

#[test]
fn gain_tokens_saved_positive_after_filtered_run() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    // We need a command with a matching filter. git/status.toml is in stdlib.
    // We'll synthesize a high-byte input via a longer string to guarantee savings.
    // Actually we just verify the JSON structure; savings can be 0 for passthrough.
    // Insert via the DB directly so we control the data.
    use tokf::tracking;
    let path = db.clone();
    let conn = tracking::open_db(&path).expect("open");
    let ev = tracking::build_event("git status", Some("git status"), 4000, 400, 5, 0);
    tracking::record_event(&conn, &ev).expect("record");
    drop(conn);

    let out = tokf_with_db(&db)
        .args(["gain", "--json"])
        .output()
        .expect("gain json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let saved = parsed["tokens_saved"].as_i64().expect("tokens_saved");
    assert!(saved > 0, "expected positive savings, got: {saved}");
}

#[test]
fn run_db_write_failure_does_not_block_output() {
    // TOKF_DB_PATH points to an impossible location; run must still print output and exit 0.
    let out = Command::new(env!("CARGO_BIN_EXE_tokf"))
        .env("TOKF_DB_PATH", "/dev/null/x/tracking.db")
        .args(["run", "echo", "hello"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "exit code: {:?}", out.status.code());
    assert!(stdout.trim() == "hello", "stdout: {stdout}");
}

#[test]
fn gain_by_filter_json_output_is_array() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    tokf_with_db(&db)
        .args(["run", "echo", "hi"])
        .output()
        .expect("run");
    let out = tokf_with_db(&db)
        .args(["gain", "--by-filter", "--json"])
        .output()
        .expect("gain by-filter json");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.is_array(), "expected array, got: {parsed}");
}
