//! E2E tests for the device auth flow: CLI → real server routes → mock GitHub.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use tokf::auth::client::{self, PollResult};
use tokf::remote::{client as machine_client, sync_client};
use tokf::tracking;

/// Full device flow: initiate → poll → receive token → verify it works.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn device_flow_creates_token(pool: PgPool) {
    let h = harness::TestHarness::with_github_mock(pool).await;
    let base_url = h.base_url.clone();

    // Step 1: Initiate device flow
    let initiate_url = base_url.clone();
    let device = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        client::initiate_device_flow(&http, &initiate_url)
    })
    .await
    .unwrap()
    .unwrap();

    assert!(!device.device_code.is_empty());
    assert_eq!(device.user_code, "TEST-1234");

    // Step 2: Poll for token (mock returns success immediately)
    let poll_url = base_url.clone();
    let device_code = device.device_code.clone();
    let result = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        client::poll_token(&http, &poll_url, &device_code)
    })
    .await
    .unwrap()
    .unwrap();

    let token_resp = match result {
        PollResult::Success(t) => t,
        other => panic!("expected PollResult::Success, got {other:?}"),
    };

    assert!(!token_resp.access_token.is_empty());
    assert_eq!(token_resp.token_type, "bearer");
    assert_eq!(token_resp.user.username, "testuser");

    // Step 3: Verify the returned token works for authenticated requests
    let verify_url = base_url;
    let new_token = token_resp.access_token;
    let gain = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_gain(&http, &verify_url, &new_token)
    })
    .await
    .unwrap()
    .unwrap();

    // New user, no events yet
    assert_eq!(gain.total_commands, 0);
}

/// Full lifecycle: device flow → token → register machine → record events → sync → gain.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn auth_then_register_machine_then_sync(pool: PgPool) {
    let h = harness::TestHarness::with_github_mock(pool).await;
    let base_url = h.base_url.clone();

    // Step 1: Device flow
    let initiate_url = base_url.clone();
    let device = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        client::initiate_device_flow(&http, &initiate_url)
    })
    .await
    .unwrap()
    .unwrap();

    let poll_url = base_url.clone();
    let dc = device.device_code.clone();
    let result = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        client::poll_token(&http, &poll_url, &dc)
    })
    .await
    .unwrap()
    .unwrap();

    let token_resp = match result {
        PollResult::Success(t) => t,
        other => panic!("expected Success, got {other:?}"),
    };
    let token = token_resp.access_token;

    // Step 2: Register machine
    let machine_id = uuid::Uuid::new_v4().to_string();
    let register_url = base_url.clone();
    let register_token = token.clone();
    let mid = machine_id.clone();
    let registered = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        machine_client::register_machine(&http, &register_url, &register_token, &mid, "e2e-host")
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(registered.machine_id, machine_id);
    assert_eq!(registered.hostname, "e2e-host");

    // Step 3: Record events in local SQLite and sync
    let conn = h.open_tracking_db();
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("h1"),
        4000,
        400,
    );
    h.record_event(
        &conn,
        "cargo test",
        Some("cargo/test"),
        Some("h2"),
        8000,
        1000,
    );

    let last_id = tracking::get_last_synced_id(&conn).unwrap();
    let events = tracking::get_events_since(&conn, last_id).unwrap();
    let sync_events: Vec<_> = events
        .iter()
        .map(|e| sync_client::SyncEvent {
            id: e.id,
            filter_name: e.filter_name.clone(),
            filter_hash: e.filter_hash.clone(),
            input_tokens: e.input_tokens_est,
            output_tokens: e.output_tokens_est,
            command_count: 1,
            recorded_at: e.timestamp.clone(),
        })
        .collect();
    let req = sync_client::SyncRequest {
        machine_id: machine_id.clone(),
        last_event_id: last_id,
        events: sync_events,
    };

    let sync_url = base_url.clone();
    let sync_token = token.clone();
    let resp = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        sync_client::sync_events(&http, &sync_url, &sync_token, &req)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.accepted, 2);

    // Step 4: Verify gain
    let gain_url = base_url;
    let gain_token = token;
    let gain = tokio::task::spawn_blocking(move || {
        let http = harness::TestHarness::http_client();
        tokf::remote::gain_client::get_gain(&http, &gain_url, &gain_token)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(gain.total_commands, 2);
    assert_eq!(gain.total_input_tokens, 3000); // 1000 + 2000
    assert_eq!(gain.total_output_tokens, 350); // 100 + 250
}
