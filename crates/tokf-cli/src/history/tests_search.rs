#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::tests::{make_record, temp_db};
use super::*;

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
