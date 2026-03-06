#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    let conn = open_db(&path).expect("open_db");
    (dir, conn)
}

#[test]
fn record_event_pipe_override_persisted() {
    let (_dir, conn) = temp_db();
    let ev = build_event(
        "cargo test",
        Some("cargo test"),
        None,
        400,
        400,
        400,
        5,
        0,
        true,
    );
    record_event(&conn, &ev).expect("record");
    let po: i64 = conn
        .query_row("SELECT pipe_override FROM events", [], |r| r.get(0))
        .expect("select");
    assert_eq!(po, 1);
}

#[test]
fn record_event_pipe_override_false_persisted() {
    let (_dir, conn) = temp_db();
    let ev = build_event(
        "cargo test",
        Some("cargo test"),
        None,
        400,
        200,
        400,
        5,
        0,
        false,
    );
    record_event(&conn, &ev).expect("record");
    let po: i64 = conn
        .query_row("SELECT pipe_override FROM events", [], |r| r.get(0))
        .expect("select");
    assert_eq!(po, 0);
}

#[test]
fn query_summary_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(
        &conn,
        &build_event("cmd1", Some("f"), None, 400, 400, 400, 5, 0, true),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("cmd2", Some("f"), None, 400, 200, 400, 5, 0, false),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("cmd3", Some("f"), None, 400, 400, 400, 5, 0, true),
    )
    .expect("record");
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.pipe_override_count, 2);
}

#[test]
fn query_by_filter_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(
        &conn,
        &build_event("cmd", Some("f1"), None, 400, 400, 400, 0, 0, true),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("cmd", Some("f1"), None, 400, 200, 400, 0, 0, false),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("cmd", Some("f2"), None, 400, 400, 400, 0, 0, true),
    )
    .expect("record");
    let rows = query_by_filter(&conn).expect("query");
    let f1 = rows.iter().find(|r| r.filter_name == "f1").expect("f1");
    let f2 = rows.iter().find(|r| r.filter_name == "f2").expect("f2");
    assert_eq!(f1.pipe_override_count, 1);
    assert_eq!(f2.pipe_override_count, 1);
}

#[test]
fn query_daily_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(
        &conn,
        &build_event("cmd", Some("f"), None, 400, 400, 400, 0, 0, true),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("cmd", Some("f"), None, 400, 200, 400, 0, 0, false),
    )
    .expect("record");
    let rows = query_daily(&conn).expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].pipe_override_count, 1);
}

/// Pre-flight check: existing DB with no write permission fails with a clear, path-bearing error.
#[test]
#[cfg(unix)]
fn open_db_readonly_file_reports_path() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    // Create the DB first, then strip write permission.
    open_db(&path).expect("initial open");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");
    let err = open_db(&path).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("cannot open"),
        "error should mention 'cannot open': {msg}"
    );
    assert!(
        msg.contains(path.to_str().unwrap()),
        "error should contain the file path: {msg}"
    );
    // Restore permissions so TempDir cleanup succeeds.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).ok();
}

#[test]
fn open_db_migrates_pipe_override_column() {
    // Simulate old schema without pipe_override, then re-open.
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    {
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(
            "CREATE TABLE events (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp         TEXT    NOT NULL,
                command           TEXT    NOT NULL,
                filter_name       TEXT,
                input_bytes       INTEGER NOT NULL,
                output_bytes      INTEGER NOT NULL,
                input_tokens_est  INTEGER NOT NULL,
                output_tokens_est INTEGER NOT NULL,
                filter_time_ms    INTEGER NOT NULL,
                exit_code         INTEGER NOT NULL
            );",
        )
        .expect("create old schema");
        conn.execute(
            "INSERT INTO events (timestamp, command, filter_name, input_bytes, output_bytes,
             input_tokens_est, output_tokens_est, filter_time_ms, exit_code)
             VALUES ('2024-01-01T00:00:00Z', 'git status', 'git status', 400, 200, 100, 50, 5, 0)",
            [],
        )
        .expect("insert old row");
    }
    // Re-open with the migration
    let conn = open_db(&path).expect("open_db with migration");
    let po: i64 = conn
        .query_row("SELECT pipe_override FROM events", [], |r| r.get(0))
        .expect("select migrated column");
    assert_eq!(po, 0, "migrated rows should default to 0");
}
