//! E2E tests for the sync data path: CLI `SQLite` → HTTP → server → `CockroachDB`.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use tokf::remote::sync_client;
use tokf::tracking;

/// Record 3 events → build sync request → POST /api/sync → assert accepted count & cursor.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn sync_records_and_uploads_events(pool: PgPool) {
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
        "git push",
        Some("git/push"),
        Some("hash2"),
        8000,
        1000,
    );
    h.record_event(
        &conn,
        "cargo test",
        Some("cargo/test"),
        Some("hash3"),
        12000,
        2000,
    );

    let req = h.build_sync_request(&conn);
    assert_eq!(req.events.len(), 3);
    assert_eq!(req.last_event_id, 0);

    let base_url = h.base_url.clone();
    let token = h.token.clone();

    let resp = tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        sync_client::sync_events(&client, &base_url, &token, &req)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.accepted, 3);
    assert_eq!(resp.cursor, 3);
}

/// Record events → sync → GET /api/gain → verify totals match.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn sync_then_gain_reflects_totals(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let conn = h.open_tracking_db();

    // input_bytes=4000 → input_tokens=1000, output_bytes=400 → output_tokens=100
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("hash1"),
        4000,
        400,
    );
    // input_bytes=8000 → input_tokens=2000, output_bytes=1000 → output_tokens=250
    h.record_event(
        &conn,
        "git push",
        Some("git/push"),
        Some("hash2"),
        8000,
        1000,
    );

    let req = h.build_sync_request(&conn);
    let base_url = h.base_url.clone();
    let token = h.token.clone();

    // Sync events
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

    assert_eq!(gain.total_input_tokens, 3000); // 1000 + 2000
    assert_eq!(gain.total_output_tokens, 350); // 100 + 250
    assert_eq!(gain.total_commands, 2);
    assert!(!gain.by_filter.is_empty());
}

/// Sync batch 1 → record more → sync batch 2 → verify gain reflects all events.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn sync_multiple_batches_advances_cursor(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let conn = h.open_tracking_db();

    // Batch 1: 2 events
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
        "git push",
        Some("git/push"),
        Some("hash2"),
        8000,
        1000,
    );

    let req = h.build_sync_request(&conn);
    assert_eq!(req.events.len(), 2);

    let base_url = h.base_url.clone();
    let token = h.token.clone();
    let resp = tokio::task::spawn_blocking({
        let base_url = base_url.clone();
        let token = token.clone();
        move || {
            let client = harness::TestHarness::http_client();
            sync_client::sync_events(&client, &base_url, &token, &req)
        }
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.accepted, 2);
    assert_eq!(resp.cursor, 2);

    // Advance local cursor
    tracking::set_last_synced_id(&conn, resp.cursor).unwrap();

    // Batch 2: 1 more event
    h.record_event(
        &conn,
        "cargo test",
        Some("cargo/test"),
        Some("hash3"),
        12000,
        2000,
    );

    let req2 = h.build_sync_request(&conn);
    assert_eq!(req2.events.len(), 1);
    assert_eq!(req2.last_event_id, 2);

    let resp2 = tokio::task::spawn_blocking({
        let base_url = base_url.clone();
        let token = token.clone();
        move || {
            let client = harness::TestHarness::http_client();
            sync_client::sync_events(&client, &base_url, &token, &req2)
        }
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp2.accepted, 1);
    assert_eq!(resp2.cursor, 3);

    // Verify gain reflects all 3 events
    let gain = tokio::task::spawn_blocking(move || {
        let client = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_gain(&client, &base_url, &token)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(gain.total_commands, 3);
    assert_eq!(gain.total_input_tokens, 6000); // 1000 + 2000 + 3000
}
