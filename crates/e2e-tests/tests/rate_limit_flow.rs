//! E2E tests for rate limiting across endpoints.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.
//! These tests create harnesses with very low rate limits and verify
//! that exceeding the limit returns a 429 error.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

use std::sync::Arc;

use tokf_server::rate_limit::{PublishRateLimiter, SyncRateLimiter};

const FILTER_TOML: &[u8] = b"command = \"git push\"\n";

fn default_test() -> (String, Vec<u8>) {
    (
        "default.toml".to_string(),
        b"name = \"default\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n".to_vec(),
    )
}

/// Sync twice successfully, third sync returns 429.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn sync_rate_limit_returns_429(pool: PgPool) {
    let h = harness::TestHarness::with_custom_state(pool, |state| {
        state.sync_rate_limiter = Arc::new(SyncRateLimiter::new(2, 3600));
    })
    .await;

    let conn = h.open_tracking_db();
    h.record_event(
        &conn,
        "git status",
        Some("git/status"),
        Some("h1"),
        4000,
        400,
    );

    let req = h.build_sync_request(&conn);

    // First two syncs should succeed
    let resp = h.blocking_sync_request(&req).await;
    assert_eq!(resp.accepted, 1);
    let resp = h.blocking_sync_request(&req).await;
    assert_eq!(resp.accepted, 0);

    // Third sync should be rate-limited
    let err = h
        .try_sync_with_token(&req, &h.token)
        .await
        .expect_err("expected rate limit error");
    let msg = err.to_string();
    assert!(
        msg.contains("429"),
        "expected 429 in error message, got: {msg}"
    );
}

/// Publish once successfully, second publish returns 429.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn publish_rate_limit_returns_429(pool: PgPool) {
    let h = harness::TestHarness::with_custom_state(pool, |state| {
        state.publish_rate_limiter = Arc::new(PublishRateLimiter::new(1, 3600));
    })
    .await;

    // First publish should succeed
    let (is_new, _) = h
        .blocking_publish(FILTER_TOML.to_vec(), vec![default_test()])
        .await;
    assert!(is_new);

    // Second publish should be rate-limited
    let err = h
        .try_publish(
            b"command = \"cargo build\"\n".to_vec(),
            vec![default_test()],
        )
        .await
        .expect_err("expected rate limit error");
    let msg = err.to_string();
    assert!(
        msg.contains("429"),
        "expected 429 in error message, got: {msg}"
    );
}

/// Search once successfully, second search returns 429.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn search_rate_limit_returns_429(pool: PgPool) {
    let h = harness::TestHarness::with_custom_state(pool, |state| {
        state.search_rate_limiter = Arc::new(PublishRateLimiter::new(1, 3600));
    })
    .await;

    // First search should succeed
    let results = h.blocking_search_filters("", 10).await;
    assert!(results.is_empty());

    // Second search should be rate-limited
    let err = h
        .try_search_filters("", 10)
        .await
        .expect_err("expected rate limit error");
    let msg = err.to_string();
    assert!(
        msg.contains("429"),
        "expected 429 in error message, got: {msg}"
    );
}

/// General rate limiter returns 429 after exceeding the limit.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn general_rate_limit_returns_429(pool: PgPool) {
    let h = harness::TestHarness::with_custom_state(pool, |state| {
        // Allow only 2 requests through the general limiter
        state.general_rate_limiter = Arc::new(PublishRateLimiter::new(2, 60));
        // Keep per-endpoint limits generous so only the general limiter fires
        state.search_rate_limiter = Arc::new(PublishRateLimiter::new(10000, 3600));
    })
    .await;

    // First two searches succeed (within general limit)
    let _ = h.blocking_search_filters("", 10).await;
    let _ = h.blocking_search_filters("", 10).await;

    // Third request exceeds the general rate limit
    let err = h
        .try_search_filters("", 10)
        .await
        .expect_err("expected rate limit error");
    let msg = err.to_string();
    assert!(
        msg.contains("429"),
        "expected 429 in error message, got: {msg}"
    );
}
