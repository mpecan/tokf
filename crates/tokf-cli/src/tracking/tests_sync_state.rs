#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use tempfile::TempDir;

// --- sync_state roundtrip ---

#[test]
fn sync_state_roundtrip() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    // Default is 0
    assert_eq!(get_last_synced_id(&conn).unwrap(), 0);
    // Set and read back
    set_last_synced_id(&conn, 42).unwrap();
    assert_eq!(get_last_synced_id(&conn).unwrap(), 42);
    // Overwrite
    set_last_synced_id(&conn, 100).unwrap();
    assert_eq!(get_last_synced_id(&conn).unwrap(), 100);
}

// --- get_events_since ---

#[test]
fn get_events_since_filters_correctly() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    // Insert some events
    for i in 0..5 {
        conn.execute(
            "INSERT INTO events (timestamp, command, filter_name, input_bytes, output_bytes,
                                 input_tokens_est, output_tokens_est, filter_time_ms, exit_code)
             VALUES ('2026-01-01T00:00:00Z', 'git status', NULL, 1000, 200, 250, 50, 10, 0)",
            [],
        )
        .unwrap();
        let _ = i;
    }
    // All 5 events have id 1-5; get events since id=2 → should return ids 3,4,5
    let events = get_events_since(&conn, 2).unwrap();
    assert_eq!(events.len(), 3);
    assert!(events[0].id > 2);
    assert!(events.iter().all(|e| e.id > 2));
}

#[test]
fn get_events_since_respects_limit_500() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();

    // Insert 600 events
    for _ in 0..600 {
        conn.execute(
            "INSERT INTO events
                (timestamp, command, filter_name, input_bytes, output_bytes,
                 input_tokens_est, output_tokens_est, filter_time_ms, exit_code)
             VALUES
                ('2026-01-01T00:00:00Z', 'git status', NULL,
                 1000, 200, 250, 50, 10, 0)",
            [],
        )
        .unwrap();
    }

    // Request all events since 0 — should be capped at 500.
    let events = get_events_since(&conn, 0).unwrap();
    assert_eq!(
        events.len(),
        500,
        "get_events_since must return at most 500 events"
    );

    // All returned IDs should be > 0 and in ascending order.
    let ids: Vec<i64> = events.iter().map(|e| e.id).collect();
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "events must be in ascending ID order"
    );

    // Request with offset inside the 600 — should return remaining (100 events: 501–600).
    let last_id = *ids.last().unwrap(); // 500
    let rest = get_events_since(&conn, last_id).unwrap();
    assert_eq!(
        rest.len(),
        100,
        "second call should return the remaining 100 events"
    );
    assert!(
        rest.iter().all(|e| e.id > last_id),
        "second batch must only contain events with id > {last_id}"
    );
}

#[test]
fn get_events_since_includes_null_filter_names() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();

    conn.execute(
        "INSERT INTO events
            (timestamp, command, filter_name, input_bytes, output_bytes,
             input_tokens_est, output_tokens_est, filter_time_ms, exit_code)
         VALUES
            ('2026-01-01T00:00:00Z', 'echo hello', NULL, 100, 50, 25, 12, 5, 0)",
        [],
    )
    .unwrap();

    let events = get_events_since(&conn, 0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].filter_name, None,
        "NULL filter_name should deserialise as None"
    );
}

// --- filter_hash ---

#[test]
fn filter_hash_stored_and_retrieved() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    let hash = "a".repeat(64); // simulate a 64-char hex hash
    let ev = build_event(
        "git status",
        Some("git status"),
        Some(&hash),
        400,
        200,
        400,
        10,
        0,
        false,
    );
    assert_eq!(ev.filter_hash.as_deref(), Some(hash.as_str()));
    record_event(&conn, &ev).expect("record");

    let events = get_events_since(&conn, 0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].filter_hash.as_deref(),
        Some(hash.as_str()),
        "filter_hash must survive the roundtrip through the DB"
    );
}

#[test]
fn filter_hash_none_stored_as_null() {
    let dir = TempDir::new().unwrap();
    let conn = open_db(&dir.path().join("test.db")).unwrap();
    let ev = build_event("echo hi", None, None, 100, 50, 100, 5, 0, false);
    assert!(ev.filter_hash.is_none());
    record_event(&conn, &ev).expect("record");

    let events = get_events_since(&conn, 0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].filter_hash, None,
        "None filter_hash should be stored as NULL and round-trip as None"
    );
}

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
        let ev = build_event("cmd", Some("f"), None, 400, 100, 400, 5, 0, false);
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
