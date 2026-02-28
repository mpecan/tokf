//! Integration tests for tokf-server.
//!
//! These tests exercise the full router stack (routing, serialisation,
//! correct HTTP status codes) and verify that the server can bind to an
//! OS-assigned port and accept connections.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tokf_server::{
    auth::mock::NoOpGitHubClient,
    rate_limit::{IpRateLimiter, PublishRateLimiter, SyncRateLimiter},
    routes::create_router,
    state::AppState,
    storage::noop::NoOpStorageClient,
};
use tokio::net::TcpListener;
use tower::ServiceExt;

/// T-4: `connect_lazy` is intentional here. The pool never actually connects
/// during router-level tests because no handler makes a real DB query (health
/// is now a liveness probe; ready is tested separately via `down_state`).
/// In production, `main.rs` uses `db::create_pool()` which connects eagerly
/// and runs migrations.
fn test_state() -> AppState {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tokf:tokf@localhost:5432/tokf_dev".to_string());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy(&url)
        .expect("invalid DATABASE_URL");
    AppState {
        db: pool,
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: true,
        public_url: "http://localhost:8080".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(100, 3600)),
        ip_search_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        ip_download_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        general_rate_limiter: Arc::new(PublishRateLimiter::new(10000, 60)),
    }
}

/// `AppState` backed by an unreachable DB for testing the 503 path.
/// Uses the RFC 2606 `.invalid` TLD which guarantees an immediate NXDOMAIN,
/// and a short `acquire_timeout` to bound the wait.
fn down_state() -> AppState {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(500))
        .connect_lazy("postgres://tokf:tokf@nonexistent-host.invalid:5432/tokf")
        .expect("lazy pool creation should not fail");
    AppState {
        db: pool,
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: true,
        public_url: "http://localhost:8080".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(100, 3600)),
        ip_search_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        ip_download_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        general_rate_limiter: Arc::new(PublishRateLimiter::new(10000, 60)),
    }
}

// ── /health (liveness) tests ─────────────────────────────────────────────────

#[tokio::test]
async fn health_always_returns_200() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_response_contains_required_fields() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok", "liveness status should always be ok");
    assert!(json["version"].is_string(), "version field missing");
    assert!(
        json.get("database").is_none(),
        "database field should not appear in liveness response"
    );
}

// ── /ready (readiness) tests ─────────────────────────────────────────────────

/// T-1: /ready must return 503 with degraded/error JSON when the DB is down.
#[tokio::test]
async fn ready_returns_503_when_db_is_down() {
    let app = create_router(down_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "degraded");
    assert_eq!(json["database"], "error");
    assert!(json["version"].is_string());
}

// ── Router-level tests (shared) ──────────────────────────────────────────────

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn post_on_health_returns_405() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn delete_on_health_returns_405() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// ── Auth route tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn get_on_auth_device_returns_405() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn auth_token_missing_body_returns_422() {
    let app = create_router(test_state());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Axum returns 400 Bad Request for an empty body with content-type: application/json
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Real TCP binding test ────────────────────────────────────────────────────

/// T-6: Bind a real TCP socket, start the server, and send an actual HTTP
/// request to verify the full network stack works end-to-end.
#[tokio::test]
async fn server_binds_to_random_port_and_accepts_http_requests() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("OS should assign a free port");
    let addr = listener
        .local_addr()
        .expect("bound socket has a local addr");

    let app = create_router(test_state());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("server should not error");
    });

    // Retry connection with bounded attempts to avoid flaky CI failures.
    let mut stream = None;
    for _ in 1..=10 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        if let Ok(s) = tokio::net::TcpStream::connect(addr).await {
            stream = Some(s);
            break;
        }
    }
    let mut stream = stream.expect("server should be reachable");

    // Send a real HTTP/1.1 GET /health request and verify a 200 response.
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .expect("write failed");

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read failed");
    let response = std::str::from_utf8(&buf).expect("invalid utf8 in response");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "expected HTTP 200 response, got: {}",
        &response[..response.len().min(80)]
    );

    handle.abort();
    handle.await.ok();
}
