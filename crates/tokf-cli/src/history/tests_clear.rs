#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::tests::{make_record, temp_db};
use super::*;

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
