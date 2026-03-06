//! E2E tests for the gain endpoints: /api/gain and /api/gain/global.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

/// Record two standard events and sync them to the server.
/// Returns `(input_tokens_total, output_tokens_total)` for assertion reference.
async fn seed_two_events(h: &harness::TestHarness) -> (i64, i64) {
    let conn = h.open_tracking_db();
    // git/status: 4000 bytes → 1000 tokens in, 400 bytes → 100 tokens out
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
    );
    // cargo/test: 8000 bytes → 2000 tokens in, 1000 bytes → 250 tokens out
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
    (3000, 350)
}

/// Fresh user with no events → GET /api/gain → all zeros.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn gain_with_no_events_returns_zeros(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;

    let gain = h.blocking_gain().await;

    assert_eq!(gain.total_input_tokens, 0);
    assert_eq!(gain.total_output_tokens, 0);
    assert_eq!(gain.total_commands, 0);
    assert_eq!(gain.total_raw_tokens, 0);
    assert!(gain.by_filter.is_empty());
    // The harness pre-creates a machine, so by_machine has 1 entry with zero totals.
    assert_eq!(gain.by_machine.len(), 1);
    assert_eq!(gain.by_machine[0].total_commands, 0);
    assert_eq!(gain.by_machine[0].total_input_tokens, 0);
    assert_eq!(gain.by_machine[0].total_output_tokens, 0);
    assert_eq!(gain.by_machine[0].total_raw_tokens, 0);
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

    // raw_tokens == input_tokens when using record_event (which passes input as raw)
    assert_eq!(
        gain.total_raw_tokens, gain.total_input_tokens,
        "raw_tokens should equal input_tokens when no baseline adjustment"
    );
}

/// Sync events → GET /api/gain/global (no auth) → verify totals.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn global_gain_returns_aggregate(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let (expected_input, _) = seed_two_events(&h).await;

    let global = h.blocking_global_gain().await;

    assert_eq!(global.total_commands, 2);
    assert_eq!(global.total_input_tokens, expected_input);
    assert_eq!(
        global.total_raw_tokens, global.total_input_tokens,
        "raw_tokens should equal input_tokens when no baseline adjustment"
    );
}

/// Sync events with raw_bytes > input_bytes → verify `total_raw_tokens` > `total_input_tokens`.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn gain_raw_tokens_propagated(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let conn = h.open_tracking_db();

    // raw_bytes (8000) > input_bytes (4000) — baseline adjustment occurred
    h.record_event_with_raw(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
        8000,
    );

    let req = h.build_sync_request(&conn);
    h.blocking_sync_request(&req).await;

    let gain = h.blocking_gain().await;

    assert_eq!(gain.total_commands, 1);
    assert_eq!(gain.total_input_tokens, 1000); // 4000 bytes ≈ 1000 tokens (÷4)
    assert!(
        gain.total_raw_tokens > gain.total_input_tokens,
        "total_raw_tokens ({}) should be > total_input_tokens ({}) when baseline adjustment occurred",
        gain.total_raw_tokens,
        gain.total_input_tokens
    );
}
