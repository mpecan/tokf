//! Integration tests for tokf-server.
//!
//! These tests exercise the full router stack (routing, serialisation,
//! correct HTTP status codes) and verify that the server can bind to an
//! OS-assigned port and accept connections.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use tokf_server::routes::create_router;
use tokio::net::TcpListener;
use tower::ServiceExt;

// ── Router-level tests (no real TCP socket) ─────────────────────────────────

#[tokio::test]
async fn health_returns_200_with_json_body() {
    let app = create_router();
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

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let app = create_router();
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
    let app = create_router();
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
    let app = create_router();
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

// ── Real TCP binding test ────────────────────────────────────────────────────

#[tokio::test]
async fn server_binds_to_random_port_and_accepts_connections() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("OS should assign a free port");
    let addr = listener
        .local_addr()
        .expect("bound socket has a local addr");

    let app = create_router();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("server should not error");
    });

    // Give the task a moment to start accepting.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let stream = tokio::net::TcpStream::connect(addr).await;
    assert!(stream.is_ok(), "server should be reachable on {addr}");

    handle.abort();
}
