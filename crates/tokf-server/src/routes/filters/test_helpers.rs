#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::mock::NoOpGitHubClient;
use crate::auth::token::{generate_token, hash_token};
use crate::rate_limit::{PublishRateLimiter, SyncRateLimiter};
use crate::state::AppState;
use crate::storage::mock::InMemoryStorageClient;

pub const BOUNDARY: &str = "tokftestboundary";
pub const MIT_ACCEPT: (&str, &[u8]) = ("mit_license_accepted", b"true");

pub fn make_state(pool: PgPool) -> AppState {
    make_state_with_storage(pool, Arc::new(InMemoryStorageClient::new()))
}

pub fn make_state_with_storage(pool: PgPool, storage: Arc<InMemoryStorageClient>) -> AppState {
    AppState {
        db: pool,
        github: Arc::new(NoOpGitHubClient),
        storage,
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: false,
        public_url: "https://registry.tokf.net".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(100, 3600)),
    }
}

pub async fn insert_test_user(pool: &PgPool, username: &str) -> (i64, String) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(rand_i64())
    .bind(username)
    .bind("https://avatars.example.com/test")
    .bind("https://github.com/test")
    .fetch_one(pool)
    .await
    .expect("failed to insert test user");

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
    .expect("failed to insert test token");

    (user_id, token)
}

pub fn rand_i64() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos(),
    ) + i64::from(rand::random::<i32>())
}

pub fn make_multipart(fields: &[(&str, &[u8])]) -> (Vec<u8>, String) {
    let mut body = Vec::new();
    for (name, content) in fields {
        let header =
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n");
        body.extend_from_slice(header.as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    (body, content_type)
}

/// POST `/api/filters` and assert success; returns the `content_hash`.
pub async fn publish_filter_helper(
    app: axum::Router,
    token: &str,
    filter_toml: &[u8],
    test_files: &[(&str, &[u8])],
) -> String {
    let mut fields: Vec<(&str, &[u8])> = vec![("filter", filter_toml), MIT_ACCEPT];
    fields.extend_from_slice(test_files);
    let (body, content_type) = make_multipart(&fields);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/filters")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "publish failed with status {}",
        resp.status()
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["content_hash"].as_str().unwrap().to_string()
}

/// GET a URI with a bearer token.
pub async fn get_request(app: axum::Router, token: &str, uri: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

/// POST `/api/filters` helper that returns the full response (for publish-specific tests).
pub async fn post_filter(
    app: axum::Router,
    token: &str,
    fields: &[(&str, &[u8])],
) -> axum::response::Response {
    let (body, content_type) = make_multipart(fields);
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/filters")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

/// PUT `/api/filters/{hash}/tests` helper that returns the full response.
pub async fn put_tests(
    app: axum::Router,
    token: &str,
    hash: &str,
    test_files: &[(&str, &[u8])],
) -> axum::response::Response {
    let mut fields: Vec<(&str, &[u8])> = Vec::new();
    for (name, content) in test_files {
        fields.push((name, content));
    }
    let (body, content_type) = make_multipart(&fields);
    app.oneshot(
        Request::builder()
            .method("PUT")
            .uri(format!("/api/filters/{hash}/tests"))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", content_type)
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .unwrap()
}

pub fn assert_status(resp: &axum::response::Response, expected: StatusCode) {
    assert_eq!(
        resp.status(),
        expected,
        "expected status {expected}, got {}",
        resp.status()
    );
}
