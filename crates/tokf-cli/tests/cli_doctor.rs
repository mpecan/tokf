#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::panic
)]

//! End-to-end CLI tests for `tokf doctor`. Each test seeds an isolated
//! `tracking.db` (via `TOKF_DB_PATH` + `TOKF_HOME`), spawns the binary,
//! and asserts on stdout / exit codes.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn tokf_with_db(db_path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    cmd.env("TOKF_DB_PATH", db_path);
    cmd.env("TOKF_HOME", db_path.parent().unwrap().join("tokf-home"));
    // Force colour off — we test against plain text.
    cmd.env("NO_COLOR", "1");
    cmd
}

fn temp_db_dir() -> TempDir {
    TempDir::new().expect("tempdir")
}

/// Open the same DB the binary will use and seed it directly. Calls
/// `open_db` so the schema (including the new `project` column and
/// burst-detection indexes) is created via the standard migration path.
fn seed_events(db_path: &Path, rows: &[(&str, &str, &str, i64, i64, i64, &str)]) {
    let conn = tokf::tracking::open_db(db_path).expect("open_db");
    for (timestamp, command, filter_name, output_bytes, raw_tokens, output_tokens, project) in rows
    {
        conn.execute(
            "INSERT INTO events
                (timestamp, command, filter_name, input_bytes, output_bytes,
                 input_tokens_est, output_tokens_est, raw_bytes, raw_tokens_est,
                 filter_time_ms, exit_code, pipe_override, project)
             VALUES
                (?1, ?2, ?3, 100, ?4, ?5, ?6, 100, ?5, 0, 0, 0, ?7)",
            rusqlite::params![
                timestamp,
                command,
                filter_name,
                output_bytes,
                raw_tokens,
                output_tokens,
                project
            ],
        )
        .expect("insert event");
    }
}

#[test]
fn doctor_empty_db_friendly_message() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    let out = tokf_with_db(&db)
        .args(["doctor", "--all"])
        .output()
        .expect("run tokf doctor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    assert!(
        stdout.contains("no events yet"),
        "expected friendly empty message, got: {stdout}"
    );
}

#[test]
fn doctor_detects_burst_pattern() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    seed_events(
        &db,
        &[
            (
                "2024-01-01T00:00:00Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:02Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:04Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:06Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:08Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
        ],
    );
    let out = tokf_with_db(&db)
        .args(["doctor", "--all"])
        .output()
        .expect("run tokf doctor");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    assert!(stdout.contains("git diff"), "stdout: {stdout}");
    assert!(stdout.contains("retry-burst detail"), "stdout: {stdout}");
    assert!(stdout.contains("×5"), "stdout: {stdout}");
}

#[test]
fn doctor_json_mode_produces_valid_json() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    seed_events(
        &db,
        &[
            (
                "2024-01-01T00:00:00Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:02Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:04Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:06Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:08Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
        ],
    );
    let out = tokf_with_db(&db)
        .args(["doctor", "--all", "--json"])
        .output()
        .expect("run tokf doctor --json");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));
    assert!(parsed["total_events_considered"].as_i64().unwrap() >= 5);
    let filters = parsed["filters"].as_array().expect("filters array");
    assert!(!filters.is_empty(), "expected at least one filter row");
    let git_diff = filters
        .iter()
        .find(|f| f["filter_name"] == "git diff")
        .expect("git diff filter");
    assert!(git_diff["burst_count"].as_i64().unwrap() >= 1);
    assert!(git_diff["max_burst_size"].as_i64().unwrap() >= 5);
}

#[test]
fn doctor_all_includes_other_projects() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    seed_events(
        &db,
        &[
            (
                "2024-01-01T00:00:00Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "/repo/a",
            ),
            (
                "2024-01-01T00:00:02Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "/repo/b",
            ),
        ],
    );
    let out_all = tokf_with_db(&db)
        .args(["doctor", "--all", "--json"])
        .output()
        .expect("run tokf doctor --all");
    let parsed: serde_json::Value = serde_json::from_slice(&out_all.stdout).unwrap();
    assert_eq!(parsed["total_events_considered"].as_i64().unwrap(), 2);
}

#[test]
fn doctor_excludes_noise_by_default() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    seed_events(
        &db,
        &[
            (
                "2024-01-01T00:00:00Z",
                "git -C /var/folders/abc/.tmpXYZ status",
                "git status",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:01Z",
                "git status",
                "git status",
                200,
                50,
                50,
                "",
            ),
        ],
    );
    let out = tokf_with_db(&db)
        .args(["doctor", "--all", "--json"])
        .output()
        .expect("run");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        parsed["total_events_considered"].as_i64().unwrap(),
        1,
        "noise event should be excluded by default"
    );

    let out_with_noise = tokf_with_db(&db)
        .args(["doctor", "--all", "--include-noise", "--json"])
        .output()
        .expect("run");
    let parsed: serde_json::Value = serde_json::from_slice(&out_with_noise.stdout).unwrap();
    assert_eq!(parsed["total_events_considered"].as_i64().unwrap(), 2);
}

#[test]
fn doctor_filter_arg_scopes_to_one_filter() {
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    seed_events(
        &db,
        &[
            (
                "2024-01-01T00:00:00Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:02Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:04Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:06Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:00:08Z",
                "git diff",
                "git diff",
                200,
                50,
                50,
                "",
            ),
            (
                "2024-01-01T00:01:00Z",
                "git status",
                "git status",
                200,
                50,
                50,
                "",
            ),
        ],
    );
    let out = tokf_with_db(&db)
        .args(["doctor", "--all", "--filter", "git diff", "--json"])
        .output()
        .expect("run");
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let filters = parsed["filters"].as_array().expect("filters array");
    assert_eq!(filters.len(), 1, "filters: {filters:?}");
    assert_eq!(filters[0]["filter_name"], "git diff");
}

#[test]
fn doctor_help_lists_burst_threshold() {
    // Sanity check: --help renders without panic and mentions a known
    // option. Catches accidental clap mis-wiring.
    let dir = temp_db_dir();
    let db = dir.path().join("tracking.db");
    let out = tokf_with_db(&db)
        .args(["doctor", "--help"])
        .output()
        .expect("run --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--burst-threshold"));
    assert!(stdout.contains("--window"));
}
