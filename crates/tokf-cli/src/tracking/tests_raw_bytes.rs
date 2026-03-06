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
fn record_event_raw_bytes_persisted() {
    let (_dir, conn) = temp_db();
    // input_bytes=200 (baseline-adjusted), raw_bytes=400 (full command output)
    let ev = build_event("cmd", Some("f"), None, 200, 50, 400, 5, 0, false);
    record_event(&conn, &ev).expect("record");
    let (rb, rt): (i64, i64) = conn
        .query_row("SELECT raw_bytes, raw_tokens_est FROM events", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("select");
    assert_eq!(rb, 400, "raw_bytes should be persisted");
    assert_eq!(rt, 100, "raw_tokens_est should be raw_bytes / 4");
}

#[test]
fn query_summary_includes_raw_tokens() {
    let (_dir, conn) = temp_db();
    // ev1: input=200B (50 tokens), raw=400B (100 tokens)
    // ev2: input=800B (200 tokens), raw=1200B (300 tokens)
    let ev1 = build_event("cmd1", Some("f"), None, 200, 50, 400, 5, 0, false);
    let ev2 = build_event("cmd2", Some("f"), None, 800, 100, 1200, 5, 0, false);
    record_event(&conn, &ev1).expect("record");
    record_event(&conn, &ev2).expect("record");
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.total_raw_tokens, 400, "total_raw_tokens: 100 + 300");
    assert_eq!(s.total_input_tokens, 250, "total_input_tokens: 50 + 200");
}

#[test]
fn open_db_migrates_raw_bytes_column() {
    // Simulate schema without raw_bytes, then re-open to trigger migration + backfill.
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
                pipe_override     INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("create old schema");
        conn.execute(
            "INSERT INTO events (timestamp, command, filter_name, input_bytes, output_bytes,
             input_tokens_est, output_tokens_est, filter_time_ms, exit_code, pipe_override)
             VALUES ('2024-01-01T00:00:00Z', 'git status', 'git status',
                     400, 200, 100, 50, 5, 0, 0)",
            [],
        )
        .expect("insert old row");
    }
    // Re-open with migration
    let conn = open_db(&path).expect("open_db with migration");
    let (rb, rt): (i64, i64) = conn
        .query_row("SELECT raw_bytes, raw_tokens_est FROM events", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("select migrated columns");
    assert_eq!(rb, 400, "raw_bytes should be backfilled from input_bytes");
    assert_eq!(
        rt, 100,
        "raw_tokens_est should be backfilled from input_tokens_est"
    );
}
