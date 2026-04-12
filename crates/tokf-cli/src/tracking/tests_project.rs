#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    let conn = open_db(&path).expect("open_db");
    (dir, conn)
}

fn count_query(conn: &Connection, sql: &str, name: &str) -> bool {
    let n: i64 = conn
        .query_row(sql, rusqlite::params![name], |r| r.get(0))
        .unwrap_or(0);
    n > 0
}

fn column_exists(conn: &Connection, name: &str) -> bool {
    count_query(
        conn,
        "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name=?1",
        name,
    )
}

fn index_exists(conn: &Connection, name: &str) -> bool {
    count_query(
        conn,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
        name,
    )
}

#[test]
fn fresh_db_has_project_column_and_indexes() {
    let (_dir, conn) = temp_db();
    assert!(
        column_exists(&conn, "project"),
        "fresh schema must include the project column"
    );
    assert!(
        index_exists(&conn, "idx_events_command_timestamp"),
        "burst-detection index must be created on fresh DBs"
    );
    assert!(
        index_exists(&conn, "idx_events_filter_timestamp"),
        "filter-detection index must be created on fresh DBs"
    );
}

#[test]
fn open_db_migrates_project_column_on_legacy_schema() {
    // Simulate the pre-#321 schema (no project column, no indexes), insert
    // a row, then re-open via open_db() which must run the migration
    // without losing existing data and add the new column with empty default.
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
                filter_hash       TEXT,
                input_bytes       INTEGER NOT NULL,
                output_bytes      INTEGER NOT NULL,
                input_tokens_est  INTEGER NOT NULL,
                output_tokens_est INTEGER NOT NULL,
                filter_time_ms    INTEGER NOT NULL,
                exit_code         INTEGER NOT NULL,
                pipe_override     INTEGER NOT NULL DEFAULT 0,
                raw_bytes         INTEGER NOT NULL DEFAULT 0,
                raw_tokens_est    INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("create old schema");
        conn.execute(
            "INSERT INTO events (timestamp, command, filter_name, input_bytes, output_bytes,
             input_tokens_est, output_tokens_est, filter_time_ms, exit_code, pipe_override)
             VALUES ('2024-01-01T00:00:00Z', 'git status', 'git/status',
                     400, 200, 100, 50, 5, 0, 0)",
            [],
        )
        .expect("insert old row");
    }
    // Re-open with migration
    let conn = open_db(&path).expect("open_db with migration");
    assert!(column_exists(&conn, "project"));
    assert!(index_exists(&conn, "idx_events_command_timestamp"));
    assert!(index_exists(&conn, "idx_events_filter_timestamp"));
    let project: String = conn
        .query_row("SELECT project FROM events", [], |r| r.get(0))
        .expect("select project");
    assert_eq!(project, "", "legacy rows must default to empty project");
}

#[test]
fn record_event_persists_project() {
    let (_dir, conn) = temp_db();
    let mut ev = build_event(
        "git status",
        Some("git/status"),
        None,
        200,
        50,
        200,
        5,
        0,
        false,
    );
    ev.project = "tokf".to_string();
    record_event(&conn, &ev).expect("record");
    let project: String = conn
        .query_row("SELECT project FROM events", [], |r| r.get(0))
        .expect("select");
    assert_eq!(project, "tokf");
}

#[test]
fn build_event_default_project_is_empty() {
    // Sanity-check the default — callers that don't set project must get
    // empty string (so old test fixtures and the e2e harness keep working).
    let ev = build_event("cmd", None, None, 100, 50, 100, 1, 0, false);
    assert_eq!(ev.project, "");
}
