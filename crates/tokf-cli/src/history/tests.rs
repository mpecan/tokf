use super::*;
use rusqlite::Connection;
use serial_test::serial;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).expect("open db");
    init_history_table(&conn).expect("init table");
    (dir, conn)
}

fn make_record(
    project: &str,
    cmd: &str,
    filter: Option<&str>,
    raw: &str,
    filtered: &str,
    ec: i32,
) -> HistoryRecord {
    HistoryRecord {
        project: project.to_owned(),
        command: cmd.to_owned(),
        filter_name: filter.map(ToOwned::to_owned),
        raw_output: raw.to_owned(),
        filtered_output: filtered.to_owned(),
        exit_code: ec,
    }
}

// --- project_root_for ---

#[test]
fn project_root_for_finds_git_dir() {
    let dir = TempDir::new().expect("tempdir");
    let real = dir.path().canonicalize().expect("canonicalize");
    std::fs::create_dir(real.join(".git")).expect("create .git");
    let subdir = real.join("src").join("components");
    std::fs::create_dir_all(&subdir).expect("create subdir");

    assert_eq!(project_root_for(&subdir), real);
}

#[test]
fn project_root_for_finds_tokf_dir() {
    let dir = TempDir::new().expect("tempdir");
    let real = dir.path().canonicalize().expect("canonicalize");
    std::fs::create_dir(real.join(".tokf")).expect("create .tokf");
    let subdir = real.join("src");
    std::fs::create_dir(&subdir).expect("create subdir");

    assert_eq!(project_root_for(&subdir), real);
}

#[test]
fn project_root_for_falls_back_to_dir() {
    let dir = TempDir::new().expect("tempdir");
    let real = dir.path().canonicalize().expect("canonicalize");
    // No .git or .tokf — should return the input dir
    assert_eq!(project_root_for(&real), real);
}

// --- init_history_table ---

#[test]
fn init_history_table_creates_table_and_indexes() {
    let (_dir, conn) = temp_db();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("query");
    assert_eq!(count, 0);

    let idx_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_history_%'",
            [],
            |r| r.get(0),
        )
        .expect("query indexes");
    assert!(
        idx_count >= 3,
        "expected at least 3 indexes (timestamp, command, project)"
    );
}

#[test]
fn init_history_table_idempotent() {
    let (_dir, conn) = temp_db();
    init_history_table(&conn).expect("second init — must not error");
}

#[test]
fn init_history_table_migrates_schema_adds_project_column() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("history.db");
    let conn = Connection::open(&path).expect("open db");

    // Simulate old schema without the project column
    conn.execute_batch(
        "CREATE TABLE history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp       TEXT    NOT NULL,
            command         TEXT    NOT NULL,
            filter_name     TEXT,
            raw_output      TEXT    NOT NULL,
            filtered_output TEXT    NOT NULL,
            exit_code       INTEGER NOT NULL
        );",
    )
    .expect("create old schema");

    init_history_table(&conn).expect("migrate");

    let has_project: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name='project'",
            [],
            |r| r.get(0),
        )
        .expect("check column");
    assert_eq!(has_project, 1, "project column must exist after migration");
}

// --- record_history ---

#[test]
fn record_history_inserts_entry() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(
        &conn,
        &make_record(
            "/proj",
            "git status",
            Some("git-status"),
            "raw output",
            "filtered output",
            0,
        ),
        &config,
    )
    .expect("record");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1);
}

#[test]
fn record_history_returns_inserted_id() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    let id1 = record_history(
        &conn,
        &make_record("/proj", "git status", None, "raw", "filtered", 0),
        &config,
    )
    .expect("record first");

    let id2 = record_history(
        &conn,
        &make_record("/proj", "cargo test", None, "raw", "filtered", 0),
        &config,
    )
    .expect("record second");

    assert!(id1 > 0, "id should be positive");
    assert!(id2 > id1, "second id should be greater than first");
}

#[test]
fn record_history_all_fields_persisted() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();
    record_history(
        &conn,
        &make_record(
            "/myproject",
            "cargo test",
            Some("cargo-test"),
            "raw test output",
            "filtered test output",
            42,
        ),
        &config,
    )
    .expect("record");

    let (proj, cmd, fname, raw, filtered, ec): (
        String,
        String,
        Option<String>,
        String,
        String,
        i32,
    ) = conn
        .query_row(
            "SELECT project, command, filter_name, raw_output, filtered_output, exit_code
             FROM history",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .expect("select");

    assert_eq!(proj, "/myproject");
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
    record_history(
        &conn,
        &make_record("/p", "cmd", None, "raw", "filtered", 0),
        &config,
    )
    .expect("record");

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
fn record_history_enforces_retention_per_project() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig { retention_count: 2 };

    // Insert 3 entries for proj-a — only 2 should remain
    for i in 1..=3 {
        record_history(
            &conn,
            &make_record("proj-a", &format!("cmd-a{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }
    // Insert 2 entries for proj-b — both should remain
    for i in 1..=2 {
        record_history(
            &conn,
            &make_record("proj-b", &format!("cmd-b{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    let proj_a = list_history(&conn, 10, Some("proj-a")).expect("list proj-a");
    assert_eq!(proj_a.len(), 2, "proj-a should have 2 (retention=2)");
    // Oldest cmd-a1 should have been pruned
    assert_eq!(proj_a[0].command, "cmd-a3");
    assert_eq!(proj_a[1].command, "cmd-a2");

    let proj_b = list_history(&conn, 10, Some("proj-b")).expect("list proj-b");
    assert_eq!(proj_b.len(), 2, "proj-b should still have 2");

    let all = list_history(&conn, 10, None).expect("list all");
    assert_eq!(all.len(), 4, "total across projects should be 4");
}

// --- list_history ---

#[test]
fn list_history_empty_db() {
    let (_dir, conn) = temp_db();
    let entries = list_history(&conn, 10, None).expect("list");
    assert_eq!(entries.len(), 0);
}

#[test]
fn list_history_returns_entries_desc() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=3 {
        record_history(
            &conn,
            &make_record("proj", &format!("cmd{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    let entries = list_history(&conn, 10, Some("proj")).expect("list");
    assert_eq!(entries.len(), 3);
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
            &make_record("proj", &format!("cmd{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    let entries = list_history(&conn, 2, Some("proj")).expect("list");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].command, "cmd5");
    assert_eq!(entries[1].command, "cmd4");
}

#[test]
fn list_history_filters_by_project() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("proj-a", "cmd1", None, "r", "f", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("proj-b", "cmd2", None, "r", "f", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("proj-a", "cmd3", None, "r", "f", 0),
        &config,
    )
    .expect("record");

    let proj_a = list_history(&conn, 10, Some("proj-a")).expect("list");
    assert_eq!(proj_a.len(), 2);
    assert!(proj_a.iter().all(|e| e.project == "proj-a"));

    let all = list_history(&conn, 10, None).expect("list all");
    assert_eq!(all.len(), 3);
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
        &make_record(
            "/repo",
            "test cmd",
            Some("test-filter"),
            "raw data",
            "filtered data",
            5,
        ),
        &config,
    )
    .expect("record");

    let id: i64 = conn
        .query_row("SELECT id FROM history LIMIT 1", [], |r| r.get(0))
        .expect("get id");

    let entry = get_history_entry(&conn, id).expect("get").expect("entry");
    assert_eq!(entry.project, "/repo");
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
    let entries = search_history(&conn, "test", 10, None).expect("search");
    assert_eq!(entries.len(), 0);
}

#[test]
fn search_history_by_command() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("p", "git status", None, "r1", "f1", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("p", "cargo test", None, "r2", "f2", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("p", "git push", None, "r3", "f3", 0),
        &config,
    )
    .expect("record");

    let entries = search_history(&conn, "git", 10, Some("p")).expect("search");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].command, "git push");
    assert_eq!(entries[1].command, "git status");
}

#[test]
fn search_history_by_raw_output() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("p", "cmd1", None, "raw with needle", "filtered1", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("p", "cmd2", None, "raw without", "filtered2", 0),
        &config,
    )
    .expect("record");

    let entries = search_history(&conn, "needle", 10, Some("p")).expect("search");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "cmd1");
}

#[test]
fn search_history_by_filtered_output() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("p", "cmd1", None, "raw1", "filtered with target", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("p", "cmd2", None, "raw2", "filtered without", 0),
        &config,
    )
    .expect("record");

    let entries = search_history(&conn, "target", 10, Some("p")).expect("search");
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
            &make_record("p", &format!("git cmd{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    let entries = search_history(&conn, "git", 2, Some("p")).expect("search");
    assert_eq!(entries.len(), 2);
}

#[test]
fn search_history_filters_by_project() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("proj-a", "git status", None, "r", "f", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("proj-b", "git push", None, "r", "f", 0),
        &config,
    )
    .expect("record");

    let proj_a = search_history(&conn, "git", 10, Some("proj-a")).expect("search");
    assert_eq!(proj_a.len(), 1);
    assert_eq!(proj_a[0].command, "git status");
}

// --- clear_history ---

#[test]
fn clear_history_removes_all_entries() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=3 {
        record_history(
            &conn,
            &make_record("p", &format!("cmd{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    clear_history(&conn, None).expect("clear");

    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count_after, 0);
}

#[test]
fn clear_history_scopes_to_project() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    record_history(
        &conn,
        &make_record("proj-a", "cmd1", None, "r", "f", 0),
        &config,
    )
    .expect("record");
    record_history(
        &conn,
        &make_record("proj-b", "cmd2", None, "r", "f", 0),
        &config,
    )
    .expect("record");

    clear_history(&conn, Some("proj-a")).expect("clear proj-a");

    let remaining = list_history(&conn, 10, None).expect("list all");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].project, "proj-b");
}

#[test]
fn clear_history_resets_autoincrement() {
    let (_dir, conn) = temp_db();
    let config = HistoryConfig::default();

    for i in 1..=3 {
        record_history(
            &conn,
            &make_record("p", &format!("before{i}"), None, "raw", "filtered", 0),
            &config,
        )
        .expect("record");
    }

    clear_history(&conn, None).expect("clear all");

    record_history(
        &conn,
        &make_record("p", "after1", None, "raw", "filtered", 0),
        &config,
    )
    .expect("record after clear");

    let id: i64 = conn
        .query_row("SELECT id FROM history LIMIT 1", [], |r| r.get(0))
        .expect("get id");
    assert_eq!(id, 1, "ID should restart from 1 after clearing all");
}

// HistoryConfig, current_project, and try_record tests live in config_tests.rs
// to keep this file under the 700-line limit.
