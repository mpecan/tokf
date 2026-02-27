#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tempfile::TempDir;

// --- last_synced_at ---

#[test]
fn last_synced_at_roundtrip() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    // Default is None
    assert_eq!(get_last_synced_at(&conn).unwrap(), None);
    // Set and read back
    set_last_synced_at(&conn, "2026-02-26T12:00:00Z").unwrap();
    assert_eq!(
        get_last_synced_at(&conn).unwrap().as_deref(),
        Some("2026-02-26T12:00:00Z")
    );
    // Overwrite
    set_last_synced_at(&conn, "2026-02-27T08:00:00Z").unwrap();
    assert_eq!(
        get_last_synced_at(&conn).unwrap().as_deref(),
        Some("2026-02-27T08:00:00Z")
    );
}

// --- pending_count ---

#[test]
fn pending_count_zero_when_empty() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    assert_eq!(get_pending_count(&conn).unwrap(), 0);
}

#[test]
fn pending_count_with_unsynced_events() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();

    // Insert 5 events
    for _ in 0..5 {
        let ev = build_event("cmd", Some("f"), None, 400, 100, 5, 0, false);
        record_event(&conn, &ev).unwrap();
    }

    // All 5 should be pending
    assert_eq!(get_pending_count(&conn).unwrap(), 5);

    // Mark 3 as synced (events 1-3)
    set_last_synced_id(&conn, 3).unwrap();
    assert_eq!(get_pending_count(&conn).unwrap(), 2);

    // Mark all as synced
    set_last_synced_id(&conn, 5).unwrap();
    assert_eq!(get_pending_count(&conn).unwrap(), 0);
}
