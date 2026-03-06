use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::mock::NoOpGitHubClient;
use crate::rate_limit::{IpRateLimiter, PublishRateLimiter, SyncRateLimiter};
use crate::routes::test_helpers::*;
use crate::state::AppState;
use crate::storage::noop::NoOpStorageClient;

use super::sync_usage;

fn app(pool: PgPool) -> Router {
    Router::new()
        .route("/api/sync", post(sync_usage))
        .with_state(make_state(pool))
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_oversized_batch(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let events: Vec<serde_json::Value> = (0..1001)
        .map(|i| {
            serde_json::json!({
                "id": i,
                "filter_name": null,
                "filter_hash": null,
                "input_tokens": 100,
                "output_tokens": 20,
                "command_count": 1,
                "recorded_at": "2026-01-01T00:00:00Z"
            })
        })
        .collect();
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": events
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_negative_input_tokens(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": null,
            "filter_hash": null,
            "input_tokens": -1,
            "output_tokens": 100,
            "command_count": 1,
            "recorded_at": "2026-01-01T00:00:00Z"
        }]
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_oversized_filter_name(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": "x".repeat(2000),
            "filter_hash": null,
            "input_tokens": 100,
            "output_tokens": 20,
            "command_count": 1,
            "recorded_at": "2026-01-01T00:00:00Z"
        }]
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_invalid_recorded_at(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": null,
            "filter_hash": null,
            "input_tokens": 100,
            "output_tokens": 20,
            "command_count": 1,
            "recorded_at": "not-a-timestamp"
        }]
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rate_limits_machine(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;

    // Use a rate limiter with limit 1
    let state = AppState {
        db: pool.clone(),
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: false,
        public_url: "http://localhost:8080".to_string(),
        terms_url: "http://localhost:8080/terms".to_string(),
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        search_rate_limiter: Arc::new(PublishRateLimiter::new(1000, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(1, 3600)),
        ip_search_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        ip_download_rate_limiter: Arc::new(IpRateLimiter::new(10000, 60)),
        general_rate_limiter: Arc::new(PublishRateLimiter::new(10000, 60)),
    };
    let app = Router::new()
        .route("/api/sync", post(sync_usage))
        .with_state(state);

    let make_req = || {
        let body = serde_json::json!({
            "machine_id": machine_id.to_string(),
            "last_event_id": 0,
            "events": []
        });
        Request::builder()
            .method("POST")
            .uri("/api/sync")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap()
    };

    // First request: ownership verified, rate check increments to 1, empty events → OK
    let resp1 = app.clone().oneshot(make_req()).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    assert!(
        resp1.headers().contains_key("x-ratelimit-limit"),
        "success response should include x-ratelimit-limit"
    );
    assert_eq!(
        resp1.headers()["x-ratelimit-remaining"],
        "0",
        "should have 0 remaining after using the single allowed request"
    );

    // Second request: rate check fails (limit=1 already consumed)
    let resp2 = app.oneshot(make_req()).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        resp2.headers().contains_key("retry-after"),
        "429 response should include retry-after"
    );
    assert_eq!(
        resp2.headers()["x-ratelimit-remaining"],
        "0",
        "429 response should have 0 remaining"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_negative_raw_tokens(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": null,
            "filter_hash": null,
            "input_tokens": 100,
            "output_tokens": 20,
            "raw_tokens": -1,
            "command_count": 1,
            "recorded_at": "2026-01-01T00:00:00Z"
        }]
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_oversized_raw_tokens(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": null,
            "filter_hash": null,
            "input_tokens": 100,
            "output_tokens": 20,
            "raw_tokens": 10_000_001,
            "command_count": 1,
            "recorded_at": "2026-01-01T00:00:00Z"
        }]
    });
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_raw_tokens_persisted(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": "git/push",
            "filter_hash": "abc123",
            "input_tokens": 500,
            "output_tokens": 100,
            "raw_tokens": 1200,
            "command_count": 1,
            "recorded_at": "2026-01-01T00:00:00Z"
        }]
    });
    let resp = app(pool.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify the raw_tokens column was persisted correctly
    let stored_raw: i64 =
        sqlx::query_scalar("SELECT raw_tokens FROM usage_events WHERE machine_id = $1")
            .bind(machine_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored_raw, 1200, "raw_tokens should be persisted as 1200");

    // Verify filter_stats aggregated total_raw_tokens
    let total_raw: i64 =
        sqlx::query_scalar("SELECT total_raw_tokens FROM filter_stats WHERE filter_hash = $1")
            .bind("abc123")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(total_raw, 1200, "total_raw_tokens should aggregate to 1200");
}
