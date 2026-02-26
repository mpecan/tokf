//! Shared test utilities for route handler tests.
//!
//! Imported in each route module's `#[cfg(test)]` block via
//! `use crate::routes::test_helpers::*;`

#![allow(clippy::unwrap_used, clippy::missing_panics_doc, clippy::panic)]

use std::sync::Arc;

use axum::http::StatusCode;
use sqlx::PgPool;

use crate::auth::mock::NoOpGitHubClient;
use crate::auth::token::{generate_token, hash_token};
use crate::rate_limit::{PublishRateLimiter, SyncRateLimiter};
use crate::state::AppState;
use crate::storage::noop::NoOpStorageClient;

/// Initialize a tracing subscriber that writes to the test output buffer.
///
/// Call at the start of any test that needs to see `tracing::error!` output.
/// Safe to call multiple times — subsequent calls are no-ops.
pub fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("tokf_server=debug")
        .try_init();
}

/// Assert an HTTP response has the expected status code and return the body.
///
/// On failure, reads and prints the response body so CI output shows the error.
pub async fn assert_status(resp: axum::response::Response, expected: StatusCode) -> bytes::Bytes {
    let actual = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 65536)
        .await
        .unwrap_or_default();
    if actual != expected {
        let body_str = String::from_utf8_lossy(&body);
        panic!(
            "assertion `left == right` failed\n  left: {actual}\n right: {expected}\n  body: {body_str}"
        );
    }
    body
}

/// Returns a unique-ish i64 suitable for use as a GitHub user ID in tests.
pub fn rand_i64() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ) + i64::from(rand::random::<i32>())
}

/// Constructs a minimal `AppState` backed by the given pool with no-op
/// GitHub and storage clients and generous rate limits.
pub fn make_state(pool: PgPool) -> AppState {
    AppState {
        db: pool,
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: false,
        public_url: "http://localhost:8080".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(100, 3600)),
    }
}

/// Inserts a test user and a non-expired auth token; returns `(user_id, token)`.
pub async fn create_user_and_token(pool: &PgPool) -> (i64, String) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES ($1, 'testuser', 'https://example.com/avatar', 'https://github.com/testuser')
         RETURNING id",
    )
    .bind(rand_i64())
    .fetch_one(pool)
    .await
    .unwrap();

    let token = generate_token();
    let token_hash = hash_token(&token);
    sqlx::query(
        "INSERT INTO auth_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, NOW() + INTERVAL '1 hour')",
    )
    .bind(user_id)
    .bind(&token_hash)
    .execute(pool)
    .await
    .unwrap();

    (user_id, token)
}

/// Inserts a test machine for the given user; returns the machine UUID.
pub async fn create_machine(pool: &PgPool, user_id: i64) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO machines (id, user_id, hostname) VALUES ($1, $2, 'test-host')")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    id
}
