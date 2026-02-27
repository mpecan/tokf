//! E2E tests for the gain endpoints: /api/gain and /api/gain/global.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use tokf::remote::sync_client;

/// Fresh user with no events → GET /api/gain → all zeros.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn gain_with_no_events_returns_zeros(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let base_url = h.base_url.clone();
    let token = h.token.clone();

    let gain = tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_gain(&client, &base_url, &token)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(gain.total_input_tokens, 0);
    assert_eq!(gain.total_output_tokens, 0);
    assert_eq!(gain.total_commands, 0);
    assert!(gain.by_filter.is_empty());
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
    let base_url = h.base_url.clone();
    let token = h.token.clone();

    // Sync
    let sync_base_url = base_url.clone();
    let sync_token = token.clone();
    tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        sync_client::sync_events(&client, &sync_base_url, &sync_token, &req).unwrap();
    })
    .await
    .unwrap();

    // Get gain
    let gain = tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_gain(&client, &base_url, &token)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(gain.total_commands, 3);
    assert!(
        gain.by_filter.len() >= 2,
        "expected at least 2 filter entries"
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
    let base_url = h.base_url.clone();
    let token = h.token.clone();

    // Sync (authenticated)
    let sync_base_url = base_url.clone();
    tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        sync_client::sync_events(&client, &sync_base_url, &token, &req).unwrap();
    })
    .await
    .unwrap();

    // Global gain (no auth required)
    let global = tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_global_gain(&client, &base_url)
    })
    .await
    .unwrap()
    .unwrap();

    // Global gain includes all users; our user's events should be included
    assert!(
        global.total_commands >= 2,
        "expected at least 2 commands in global gain, got {}",
        global.total_commands
    );
    assert!(global.total_input_tokens >= 3000);
}
