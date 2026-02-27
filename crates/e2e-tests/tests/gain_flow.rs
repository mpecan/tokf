//! E2E tests for the gain endpoints: /api/gain and /api/gain/global.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

/// Fresh user with no events → GET /api/gain → all zeros.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn gain_with_no_events_returns_zeros(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;

    let gain = h.blocking_gain().await;

    assert_eq!(gain.total_input_tokens, 0);
    assert_eq!(gain.total_output_tokens, 0);
    assert_eq!(gain.total_commands, 0);
    assert!(gain.by_filter.is_empty());
    assert!(
        gain.by_machine.is_empty(),
        "expected empty by_machine for fresh user, got {} entries",
        gain.by_machine.len()
    );
}

/// Sync events with different filter names → verify `by_filter` breakdown.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn gain_by_filter_breakdown(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let conn = h.open_tracking_db();

    // Two events for git/status
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
    );
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
    );
    // One event for cargo/test
    h.record_event(
        &conn,
        "cargo test",
        Some("cargo/test"),
        Some("hash2"),
        12000,
        2000,
    );

    let req = h.build_sync_request(&conn);
    h.blocking_sync_request(&req).await;

    let gain = h.blocking_gain().await;

    assert_eq!(gain.total_commands, 3);
    assert_eq!(
        gain.by_filter.len(),
        2,
        "expected exactly 2 filter entries, got {}",
        gain.by_filter.len()
    );

    let git_status = gain
        .by_filter
        .iter()
        .find(|f| f.filter_name.as_deref() == Some("git/status"));
    assert!(git_status.is_some(), "expected git/status in by_filter");
    assert_eq!(git_status.unwrap().total_commands, 2);

    let cargo_test = gain
        .by_filter
        .iter()
        .find(|f| f.filter_name.as_deref() == Some("cargo/test"));
    assert!(cargo_test.is_some(), "expected cargo/test in by_filter");
    assert_eq!(cargo_test.unwrap().total_commands, 1);
}

/// Sync events → GET /api/gain/global (no auth) → verify totals.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn global_gain_returns_aggregate(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let conn = h.open_tracking_db();

    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
    );
    h.record_event(
        &conn,
        "cargo test",
        Some("cargo/test"),
        Some("hash2"),
        8000,
        1000,
    );

    let req = h.build_sync_request(&conn);
    h.blocking_sync_request(&req).await;

    let global = h.blocking_global_gain().await;

    // Per-test DB isolation means our events are the only ones present
    assert_eq!(
        global.total_commands, 2,
        "expected exactly 2 commands in global gain, got {}",
        global.total_commands
    );
    assert_eq!(global.total_input_tokens, 3000); // 1000 + 2000
}
