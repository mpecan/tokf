//! E2E tests for the device auth flow: CLI → real server routes → mock GitHub.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use tokf::auth::client::{self, PollResult};
use tokf::remote::client as machine_client;
use tokf::remote::http::Client;

/// Build a raw reqwest blocking client for the device auth flow.
/// The auth flow uses raw reqwest since it's interactive polling,
/// not the standard Client wrapper.
fn auth_http_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

/// Full device flow: initiate → poll → receive token → verify it works.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn device_flow_creates_token(pool: PgPool) {
    let h = harness::TestHarness::with_github_mock(pool).await;
    let base_url = h.base_url.clone();

    // Step 1: Initiate device flow
    let initiate_url = base_url.clone();
    let device = tokio::task::spawn_blocking(move || {
        let http = auth_http_client();
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
        let http = auth_http_client();
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
    let gain = h.blocking_gain_with_token(&token_resp.access_token).await;

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
        let http = auth_http_client();
        client::initiate_device_flow(&http, &initiate_url)
    })
    .await
    .unwrap()
    .unwrap();

    let poll_url = base_url.clone();
    let dc = device.device_code.clone();
    let result = tokio::task::spawn_blocking(move || {
        let http = auth_http_client();
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
    let register_base = base_url.clone();
    let register_token = token.clone();
    let mid = machine_id.clone();
    let registered = tokio::task::spawn_blocking(move || {
        let client = Client::new(&register_base, Some(&register_token)).unwrap();
        machine_client::register_machine(&client, &mid, "e2e-host")
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

    let req = h.build_sync_request_for_machine(&conn, &machine_id);

    let sync_base = base_url.clone();
    let sync_token = token.clone();
    let resp = tokio::task::spawn_blocking(move || {
        let client = Client::new(&sync_base, Some(&sync_token)).unwrap();
        tokf::remote::sync_client::sync_events(&client, &req)
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(resp.accepted, 2);

    // Step 4: Verify gain (using the device-flow token, not the harness token)
    let gain = h.blocking_gain_with_token(&token).await;

    assert_eq!(gain.total_commands, 2);
    assert_eq!(gain.total_input_tokens, 3000); // 1000 + 2000
    assert_eq!(gain.total_output_tokens, 350); // 100 + 250
}
