use super::*;
use rusqlite::Connection;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).expect("open db");
    init_history_table(&conn).expect("init table");
    (dir, conn)
}

// --- init_history_table ---

#[test]
fn init_history_table_creates_table_and_indexes() {
    let (_dir, conn) = temp_db();
    // Verify table exists
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("query");
    assert_eq!(count, 0);

    // Verify indexes exist
    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_history_%'",
            [],
            |r| r.get(0),
        )
        .expect("query indexes");
    assert!(idx_count >= 2, "expected at least 2 indexes");
}

#[test]
fn init_history_table_idempotent() {
    let (_dir, conn) = temp_db();
    init_history_table(&conn).expect("second init — must not error");
}

// --- record_history ---

#[test]
fn record_history_inserts_entry() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(
        &conn,
        "git status",
        Some("git-status"),
        "raw output",
        "filtered output",
        0,
        &config,
    )
    .expect("record");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1);
}

#[test]
fn record_history_all_fields_persisted() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(
        &conn,
        "cargo test",
        Some("cargo-test"),
        "raw test output",
        "filtered test output",
        42,
        &config,
    )
    .expect("record");

    let (cmd, fname, raw, filtered, ec): (String, Option<String>, String, String, i32) = conn
        .query_row(
            "SELECT command, filter_name, raw_output, filtered_output, exit_code FROM history",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .expect("select");

    assert_eq!(cmd, "cargo test");
    assert_eq!(fname.as_deref(), Some("cargo-test"));
    assert_eq!(raw, "raw test output");
    assert_eq!(filtered, "filtered test output");
    assert_eq!(ec, 42);
}

#[test]
fn record_history_timestamp_iso8601() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(&conn, "cmd", None, "raw", "filtered", 0, &config).expect("record");

    let ts: String = conn
        .query_row("SELECT timestamp FROM history", [], |r| r.get(0))
        .expect("ts");
    assert!(ts.len() >= 10, "timestamp too short: {ts}");
    let date_part = &ts[..10];
    assert!(
        date_part.chars().nth(4) == Some('-') && date_part.chars().nth(7) == Some('-'),
        "bad ISO date: {ts}"
    );
}

#[test]
fn record_history_enforces_retention_limit() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig { retention_count: 3 };

    // Insert 5 entries
    for i in 1..=5 {
        record_history(
            &conn,
            &format!("cmd{i}"),
            None,
            "raw",
            "filtered",
            0,
            &config,
        )
        .expect("record");
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 3, "should only keep last 3 entries");

    // Verify the oldest entries were deleted (should keep most recent 3 by ID)
    let commands: Vec<String> = conn
        .prepare("SELECT command FROM history ORDER BY id ASC")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");

    assert_eq!(commands, vec!["cmd3", "cmd4", "cmd5"]);
}

// --- list_history ---

#[test]
fn list_history_empty_db() {
    let (_dir, conn) = temp_db();
    let entries = list_history(&conn, 10).expect("list");
    assert_eq!(entries.len(), 0);
}

#[test]
fn list_history_returns_entries_desc() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=3 {
        record_history(
            &conn,
            &format!("cmd{i}"),
            None,
            "raw",
            "filtered",
            0,
            &config,
        )
        .expect("record");
    }

    let entries = list_history(&conn, 10).expect("list");
    assert_eq!(entries.len(), 3);
    // Most recent first
    assert_eq!(entries[0].command, "cmd3");
    assert_eq!(entries[1].command, "cmd2");
    assert_eq!(entries[2].command, "cmd1");
}

#[test]
fn list_history_respects_limit() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=5 {
        record_history(
            &conn,
            &format!("cmd{i}"),
            None,
            "raw",
            "filtered",
            0,
            &config,
        )
        .expect("record");
    }

    let entries = list_history(&conn, 2).expect("list");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].command, "cmd5");
    assert_eq!(entries[1].command, "cmd4");
}

// --- get_history_entry ---

#[test]
fn get_history_entry_not_found() {
    let (_dir, conn) = temp_db();
    let entry = get_history_entry(&conn, 999).expect("get");
    assert!(entry.is_none());
}

#[test]
fn get_history_entry_found() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(
        &conn,
        "test cmd",
        Some("test-filter"),
        "raw data",
        "filtered data",
        5,
        &config,
    )
    .expect("record");

    let id: i64 = conn
        .query_row("SELECT id FROM history LIMIT 1", [], |r| r.get(0))
        .expect("get id");

    let entry = get_history_entry(&conn, id).expect("get").expect("entry");
    assert_eq!(entry.command, "test cmd");
    assert_eq!(entry.filter_name.as_deref(), Some("test-filter"));
    assert_eq!(entry.raw_output, "raw data");
    assert_eq!(entry.filtered_output, "filtered data");
    assert_eq!(entry.exit_code, 5);
}

// --- search_history ---

#[test]
fn search_history_empty_db() {
    let (_dir, conn) = temp_db();
    let entries = search_history(&conn, "test", 10).expect("search");
    assert_eq!(entries.len(), 0);
}

#[test]
fn search_history_by_command() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(&conn, "git status", None, "raw1", "filtered1", 0, &config).expect("record");
    record_history(&conn, "cargo test", None, "raw2", "filtered2", 0, &config).expect("record");
    record_history(&conn, "git push", None, "raw3", "filtered3", 0, &config).expect("record");

    let entries = search_history(&conn, "git", 10).expect("search");
    assert_eq!(entries.len(), 2);
    // Most recent first
    assert_eq!(entries[0].command, "git push");
    assert_eq!(entries[1].command, "git status");
}

#[test]
fn search_history_by_raw_output() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        "cmd1",
        None,
        "raw with needle",
        "filtered1",
        0,
        &config,
    )
    .expect("record");
    record_history(&conn, "cmd2", None, "raw without", "filtered2", 0, &config).expect("record");

    let entries = search_history(&conn, "needle", 10).expect("search");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "cmd1");
}

#[test]
fn search_history_by_filtered_output() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        "cmd1",
        None,
        "raw1",
        "filtered with target",
        0,
        &config,
    )
    .expect("record");
    record_history(&conn, "cmd2", None, "raw2", "filtered without", 0, &config).expect("record");

    let entries = search_history(&conn, "target", 10).expect("search");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "cmd1");
}

#[test]
fn search_history_respects_limit() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=5 {
        record_history(
            &conn,
            &format!("git cmd{i}"),
            None,
            "raw",
            "filtered",
            0,
            &config,
        )
        .expect("record");
    }

    let entries = search_history(&conn, "git", 2).expect("search");
    assert_eq!(entries.len(), 2);
}

// --- clear_history ---

#[test]
fn clear_history_removes_all_entries() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=3 {
        record_history(
            &conn,
            &format!("cmd{i}"),
            None,
            "raw",
            "filtered",
            0,
            &config,
        )
        .expect("record");
    }

    let count_before: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count_before, 3);

    clear_history(&conn).expect("clear");

    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count_after, 0);
}

// --- HistoryConfig ---

#[test]
fn history_config_default_retention() {
    let config = HistoryConfig::default();
    assert_eq!(config.retention_count, 10);
}
