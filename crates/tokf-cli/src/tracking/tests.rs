#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serial_test::serial;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    let conn = open_db(&path).expect("open_db");
    (dir, conn)
}

// --- db_path / open_db ---

/// Must run serially: mutates the global process environment.
#[test]
#[serial]
fn db_path_env_override() {
    let dir = TempDir::new().expect("tempdir");
    let custom = dir.path().join("custom.db");
    // SAFETY: test-only env mutation; #[serial] prevents races with other tests.
    unsafe {
        std::env::set_var("TOKF_DB_PATH", custom.to_str().expect("str"));
    }
    let result = db_path();
    unsafe {
        std::env::remove_var("TOKF_DB_PATH");
    }
    assert_eq!(result, Some(custom));
}

#[test]
#[serial]
fn db_path_tokf_home_override() {
    let dir = TempDir::new().expect("tempdir");
    // SAFETY: test-only env mutation; #[serial] prevents races with other tests.
    unsafe {
        std::env::set_var("TOKF_HOME", dir.path().to_str().expect("str"));
    }
    let result = db_path();
    unsafe {
        std::env::remove_var("TOKF_HOME");
    }
    assert_eq!(result, Some(dir.path().join("tracking.db")));
}

/// `TOKF_DB_PATH` must take priority over `TOKF_HOME`.
#[test]
#[serial]
fn db_path_tokf_db_path_wins_over_tokf_home() {
    let dir = TempDir::new().expect("tempdir");
    let custom = dir.path().join("custom.db");
    let home_dir = TempDir::new().expect("tempdir");
    // SAFETY: test-only env mutation; #[serial] prevents races with other tests.
    unsafe {
        std::env::set_var("TOKF_DB_PATH", custom.to_str().expect("str"));
        std::env::set_var("TOKF_HOME", home_dir.path().to_str().expect("str"));
    }
    let result = db_path();
    unsafe {
        std::env::remove_var("TOKF_DB_PATH");
        std::env::remove_var("TOKF_HOME");
    }
    assert_eq!(result, Some(custom));
}

#[test]
fn open_db_creates_dir_and_schema() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("sub").join("tracking.db");
    let conn = open_db(&path).expect("open_db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("query");
    assert_eq!(count, 0);
}

#[test]
fn open_db_idempotent() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    open_db(&path).expect("first open");
    open_db(&path).expect("second open — must not error");
}

// --- record_event ---

#[test]
fn record_event_inserts_row() {
    let (_dir, conn) = temp_db();
    let ev = build_event("echo hi", None, 100, 50, 5, 0, false);
    record_event(&conn, &ev).expect("record");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1);
}

#[test]
fn record_event_all_fields_persisted() {
    let (_dir, conn) = temp_db();
    let ev = build_event("git status", Some("git status"), 400, 200, 10, 0, false);
    record_event(&conn, &ev).expect("record");
    let (cmd, fname, ib, ob, it, ot, ft, ec): (
        String,
        Option<String>,
        i64,
        i64,
        i64,
        i64,
        i64,
        i32,
    ) = conn
        .query_row(
            "SELECT command, filter_name, input_bytes, output_bytes,
                     input_tokens_est, output_tokens_est,
                     filter_time_ms, exit_code
              FROM events",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .expect("select");
    assert_eq!(cmd, "git status");
    assert_eq!(fname.as_deref(), Some("git status"));
    assert_eq!(ib, 400);
    assert_eq!(ob, 200);
    assert_eq!(it, 100); // 400 / 4
    assert_eq!(ot, 50); // 200 / 4
    assert_eq!(ft, 10);
    assert_eq!(ec, 0);
}

/// C1+C2: verify `exit_code` and `filter_time_ms` are readable with non-zero values.
#[test]
fn record_event_exit_code_and_filter_time_persisted() {
    let (_dir, conn) = temp_db();
    // exit_code = 42 (non-zero), filter_time_ms = 99
    let ev = build_event("cargo test", Some("cargo test"), 800, 200, 99, 42, false);
    record_event(&conn, &ev).expect("record");
    let (ft, ec): (i64, i32) = conn
        .query_row("SELECT filter_time_ms, exit_code FROM events", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .expect("select");
    assert_eq!(ft, 99, "filter_time_ms not persisted correctly");
    assert_eq!(ec, 42, "exit_code not persisted correctly");
}

#[test]
fn record_event_timestamp_iso8601() {
    let (_dir, conn) = temp_db();
    let ev = build_event("cmd", None, 0, 0, 0, 0, false);
    record_event(&conn, &ev).expect("record");
    let ts: String = conn
        .query_row("SELECT timestamp FROM events", [], |r| r.get(0))
        .expect("ts");
    assert!(ts.len() >= 10, "timestamp too short: {ts}");
    let date_part = &ts[..10];
    assert!(
        date_part.chars().nth(4) == Some('-') && date_part.chars().nth(7) == Some('-'),
        "bad ISO date: {ts}"
    );
}

// --- build_event ---

#[test]
fn build_event_token_estimation() {
    let ev = build_event("x", None, 400, 0, 0, 0, false);
    assert_eq!(ev.input_tokens_est, 100);
    let ev2 = build_event("x", None, 399, 0, 0, 0, false);
    assert_eq!(ev2.input_tokens_est, 99);
}

#[test]
fn build_event_passthrough_filter_name_none() {
    let ev = build_event("echo hi", None, 10, 10, 0, 0, false);
    assert!(ev.filter_name.is_none());
}

// --- query_summary ---

#[test]
fn query_summary_empty_db() {
    let (_dir, conn) = temp_db();
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.total_commands, 0);
    assert_eq!(s.total_input_tokens, 0);
    assert_eq!(s.total_output_tokens, 0);
    assert_eq!(s.tokens_saved, 0);
    assert!(s.savings_pct.abs() < f64::EPSILON);
    assert_eq!(s.pipe_override_count, 0);
}

#[test]
fn query_summary_with_events() {
    let (_dir, conn) = temp_db();
    // input_tokens 100, output_tokens 25 → saved 75
    let ev = build_event("cmd", Some("f"), 400, 100, 5, 0, false);
    record_event(&conn, &ev).expect("record");
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.total_commands, 1);
    assert_eq!(s.total_input_tokens, 100);
    assert_eq!(s.total_output_tokens, 25);
    assert_eq!(s.tokens_saved, 75);
    assert!((s.savings_pct - 75.0).abs() < 0.01);
}

#[test]
fn query_summary_zero_input_no_divide_by_zero() {
    let (_dir, conn) = temp_db();
    let ev = build_event("cmd", None, 0, 0, 0, 0, false);
    record_event(&conn, &ev).expect("record");
    let s = query_summary(&conn).expect("summary");
    assert!(s.savings_pct.abs() < f64::EPSILON); // must not panic or NaN
}

/// C3: multiple events with diverse byte counts — verify correct accumulation.
#[test]
fn query_summary_accumulates_multiple_events() {
    let (_dir, conn) = temp_db();
    // ev1: 400 in → 100 tokens, 100 out → 25 tokens, saved 75
    // ev2: 800 in → 200 tokens, 400 out → 100 tokens, saved 100
    // ev3: 1200 in → 300 tokens,   0 out →  0 tokens, saved 300
    // totals: 3 commands, 600 input, 125 output, 475 saved ≈ 79.17%
    let events = [
        build_event("cmd1", Some("f1"), 400, 100, 5, 0, false),
        build_event("cmd2", Some("f2"), 800, 400, 10, 1, false),
        build_event("cmd3", None, 1200, 0, 0, 0, false),
    ];
    for ev in &events {
        record_event(&conn, ev).expect("record");
    }
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.total_commands, 3);
    assert_eq!(s.total_input_tokens, 600); // (400+800+1200)/4
    assert_eq!(s.total_output_tokens, 125); // (100+400+0)/4
    assert_eq!(s.tokens_saved, 475); // 600-125
    assert!((s.savings_pct - 79.166_666).abs() < 0.01);
}

// --- query_by_filter ---

#[test]
fn query_by_filter_groups_correctly() {
    let (_dir, conn) = temp_db();
    for fname in &["alpha", "beta", "gamma"] {
        let ev = build_event("cmd", Some(fname), 400, 100, 0, 0, false);
        record_event(&conn, &ev).expect("record");
    }
    let rows = query_by_filter(&conn).expect("query");
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.commands == 1));
}

#[test]
fn query_by_filter_null_shown_as_passthrough() {
    let (_dir, conn) = temp_db();
    let ev = build_event("echo hi", None, 200, 200, 0, 0, false);
    record_event(&conn, &ev).expect("record");
    let rows = query_by_filter(&conn).expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].filter_name, "passthrough");
}

/// Verify that named-filter rows and passthrough (NULL) rows coexist correctly.
#[test]
fn query_by_filter_mixed_null_and_named() {
    let (_dir, conn) = temp_db();
    record_event(
        &conn,
        &build_event("git status", Some("git status"), 400, 100, 5, 0, false),
    )
    .expect("record");
    record_event(&conn, &build_event("echo hi", None, 200, 200, 0, 0, false)).expect("record");
    let rows = query_by_filter(&conn).expect("query");
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|r| r.filter_name.as_str()).collect();
    assert!(names.contains(&"git status"), "rows: {names:?}");
    assert!(names.contains(&"passthrough"), "rows: {names:?}");
}

/// Verify ordering: filter with greater token savings appears first.
#[test]
fn query_by_filter_ordered_by_savings_desc() {
    let (_dir, conn) = temp_db();
    // "small": 100 in → 25 tokens, 80 out → 20 tokens, saved 5
    // "big":   400 in → 100 tokens,  0 out →  0 tokens, saved 100
    record_event(
        &conn,
        &build_event("cmd", Some("small"), 100, 80, 0, 0, false),
    )
    .expect("record");
    record_event(&conn, &build_event("cmd", Some("big"), 400, 0, 0, 0, false)).expect("record");
    let rows = query_by_filter(&conn).expect("query");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].filter_name, "big",
        "highest savings should be first"
    );
    assert_eq!(rows[1].filter_name, "small");
}

// --- query_daily ---

#[test]
fn query_daily_groups_by_date() {
    let (_dir, conn) = temp_db();
    for _ in 0..2 {
        let ev = build_event("cmd", None, 400, 100, 0, 0, false);
        record_event(&conn, &ev).expect("record");
    }
    let rows = query_daily(&conn).expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].commands, 2);
}

// --- pipe_override ---

#[test]
fn record_event_pipe_override_persisted() {
    let (_dir, conn) = temp_db();
    let ev = build_event("cargo test", Some("cargo test"), 400, 400, 5, 0, true);
    record_event(&conn, &ev).expect("record");
    let po: i64 = conn
        .query_row("SELECT pipe_override FROM events", [], |r| r.get(0))
        .expect("select");
    assert_eq!(po, 1);
}

#[test]
fn record_event_pipe_override_false_persisted() {
    let (_dir, conn) = temp_db();
    let ev = build_event("cargo test", Some("cargo test"), 400, 200, 5, 0, false);
    record_event(&conn, &ev).expect("record");
    let po: i64 = conn
        .query_row("SELECT pipe_override FROM events", [], |r| r.get(0))
        .expect("select");
    assert_eq!(po, 0);
}

#[test]
fn query_summary_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(&conn, &build_event("cmd1", Some("f"), 400, 400, 5, 0, true)).expect("record");
    record_event(
        &conn,
        &build_event("cmd2", Some("f"), 400, 200, 5, 0, false),
    )
    .expect("record");
    record_event(&conn, &build_event("cmd3", Some("f"), 400, 400, 5, 0, true)).expect("record");
    let s = query_summary(&conn).expect("summary");
    assert_eq!(s.pipe_override_count, 2);
}

#[test]
fn query_by_filter_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(&conn, &build_event("cmd", Some("f1"), 400, 400, 0, 0, true)).expect("record");
    record_event(
        &conn,
        &build_event("cmd", Some("f1"), 400, 200, 0, 0, false),
    )
    .expect("record");
    record_event(&conn, &build_event("cmd", Some("f2"), 400, 400, 0, 0, true)).expect("record");
    let rows = query_by_filter(&conn).expect("query");
    let f1 = rows.iter().find(|r| r.filter_name == "f1").expect("f1");
    let f2 = rows.iter().find(|r| r.filter_name == "f2").expect("f2");
    assert_eq!(f1.pipe_override_count, 1);
    assert_eq!(f2.pipe_override_count, 1);
}

#[test]
fn query_daily_pipe_override_count() {
    let (_dir, conn) = temp_db();
    record_event(&conn, &build_event("cmd", Some("f"), 400, 400, 0, 0, true)).expect("record");
    record_event(&conn, &build_event("cmd", Some("f"), 400, 200, 0, 0, false)).expect("record");
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
