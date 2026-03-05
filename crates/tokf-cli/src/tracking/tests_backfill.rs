#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tempfile::TempDir;

fn temp_db() -> (TempDir, Connection) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("tracking.db");
    let conn = open_db(&path).expect("open_db");
    (dir, conn)
}

fn make_backfill_filter(
    command: &str,
    relative: &str,
    hash: &str,
) -> crate::config::ResolvedFilter {
    use std::path::PathBuf;
    use tokf_common::config::types::FilterConfig;
    crate::config::ResolvedFilter {
        config: toml::from_str::<FilterConfig>(&format!("command = \"{command}\"")).unwrap(),
        hash: hash.to_owned(),
        source_path: PathBuf::from(format!("{relative}.toml")),
        relative_path: PathBuf::from(format!("{relative}.toml")),
        priority: 255,
    }
}

#[test]
fn backfill_updates_known_filters() {
    let (_dir, conn) = temp_db();
    // Two filtered events (no hash) + one passthrough event (no filter_name)
    record_event(
        &conn,
        &build_event(
            "git status",
            Some("git/status"),
            None,
            400,
            200,
            10,
            0,
            false,
        ),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event(
            "cargo test",
            Some("cargo/test"),
            None,
            400,
            200,
            10,
            0,
            false,
        ),
    )
    .expect("record");
    record_event(
        &conn,
        &build_event("echo hi", None, None, 100, 50, 5, 0, false),
    )
    .expect("record");

    let hash1 = "a".repeat(64);
    let hash2 = "b".repeat(64);
    let filters = vec![
        make_backfill_filter("git status", "git/status", &hash1),
        make_backfill_filter("cargo test", "cargo/test", &hash2),
    ];

    let (updated, not_found) = backfill_filter_hashes(&conn, &filters).expect("backfill");
    assert_eq!(updated, 2, "two events should be updated");
    assert!(not_found.is_empty(), "no unknown filters: {not_found:?}");

    let events = get_events_since(&conn, 0).unwrap();
    let git = events
        .iter()
        .find(|e| e.filter_name.as_deref() == Some("git/status"))
        .unwrap();
    let cargo = events
        .iter()
        .find(|e| e.filter_name.as_deref() == Some("cargo/test"))
        .unwrap();
    assert_eq!(git.filter_hash.as_deref(), Some(hash1.as_str()));
    assert_eq!(cargo.filter_hash.as_deref(), Some(hash2.as_str()));
    // passthrough event must still have no hash
    let passthrough = events.iter().find(|e| e.filter_name.is_none()).unwrap();
    assert!(passthrough.filter_hash.is_none());
}

#[test]
fn backfill_reports_not_found_names() {
    let (_dir, conn) = temp_db();
    record_event(
        &conn,
        &build_event("old cmd", Some("old/cmd"), None, 400, 200, 10, 0, false),
    )
    .expect("record");

    let filters = vec![make_backfill_filter(
        "git status",
        "git/status",
        &"a".repeat(64),
    )];
    let (updated, not_found) = backfill_filter_hashes(&conn, &filters).expect("backfill");
    assert_eq!(updated, 0);
    assert_eq!(not_found, vec!["old/cmd"]);
}

#[test]
fn backfill_skips_already_hashed_events() {
    let (_dir, conn) = temp_db();
    let existing_hash = "z".repeat(64);
    record_event(
        &conn,
        &build_event(
            "git status",
            Some("git/status"),
            Some(&existing_hash),
            400,
            200,
            10,
            0,
            false,
        ),
    )
    .expect("record");

    // different hash — should not overwrite the existing one
    let filters = vec![make_backfill_filter(
        "git status",
        "git/status",
        &"a".repeat(64),
    )];
    let (updated, not_found) = backfill_filter_hashes(&conn, &filters).expect("backfill");
    assert_eq!(updated, 0, "already-hashed event must not be updated");
    assert!(not_found.is_empty());

    let events = get_events_since(&conn, 0).unwrap();
    assert_eq!(
        events[0].filter_hash.as_deref(),
        Some(existing_hash.as_str()),
        "existing hash must be preserved"
    );
}

#[test]
fn filter_hash_migration() {
    // Simulate old schema without filter_hash, then re-open to trigger migration.
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
                exit_code         INTEGER NOT NULL,
                pipe_override     INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("create old schema");
        conn.execute(
            "INSERT INTO events
                (timestamp, command, filter_name, input_bytes, output_bytes,
                 input_tokens_est, output_tokens_est, filter_time_ms, exit_code, pipe_override)
             VALUES
                ('2024-01-01T00:00:00Z', 'git status', 'git status',
                 400, 200, 100, 50, 5, 0, 0)",
            [],
        )
        .expect("insert old row");
    }
    // Re-open with migration
    let conn = open_db(&path).expect("open_db with migration");
    // Verify migration ran: filter_hash column exists and old rows have NULL
    let fh: Option<String> = conn
        .query_row("SELECT filter_hash FROM events", [], |r| r.get(0))
        .expect("select migrated column");
    assert_eq!(fh, None, "migrated rows should have NULL filter_hash");
}
