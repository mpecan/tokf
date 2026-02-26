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
use crate::rate_limit::{PublishRateLimiter, SyncRateLimiter};
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
async fn sync_requires_auth(pool: PgPool) {
    let app = app(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"machine_id":"00000000-0000-0000-0000-000000000000","last_event_id":0,"events":[]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_invalid_uuid(pool: PgPool) {
    let (_, token) = create_user_and_token(&pool).await;
    let app = app(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sync")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"machine_id":"not-a-uuid","last_event_id":0,"events":[]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_unowned_machine(pool: PgPool) {
    let (_, token) = create_user_and_token(&pool).await;
    let (other_user_id, _) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, other_user_id).await;
    let app = app(pool);
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": []
    });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_rejects_nonexistent_machine(pool: PgPool) {
    let (_, token) = create_user_and_token(&pool).await;
    let body = serde_json::json!({
        "machine_id": "00000000-0000-0000-0000-000000000099",
        "last_event_id": 0,
        "events": []
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
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_accepts_empty_batch(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let app = app(pool);
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": []
    });
    let resp = app
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["accepted"], 0);
    assert_eq!(result["cursor"], 0);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_inserts_events_and_advances_cursor(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let app = app(pool.clone());
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [
            {
                "id": 1,
                "filter_name": "git/push",
                "filter_hash": null,
                "input_tokens": 1000,
                "output_tokens": 200,
                "command_count": 1,
                "recorded_at": "2026-01-01T00:00:00Z"
            },
            {
                "id": 2,
                "filter_name": "cargo/test",
                "filter_hash": null,
                "input_tokens": 500,
                "output_tokens": 100,
                "command_count": 1,
                "recorded_at": "2026-01-01T00:01:00Z"
            },
        ]
    });
    let resp = app
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
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["accepted"], 2);
    assert_eq!(result["cursor"], 2);

    // Verify last_sync_at was updated
    let last_sync: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_sync_at FROM machines WHERE id = $1")
            .bind(machine_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(last_sync.is_some());
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_deduplicates_via_cursor(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;

    let make_req = |events: serde_json::Value| {
        let token = token.clone();
        let body = serde_json::json!({
            "machine_id": machine_id.to_string(),
            "last_event_id": 0,
            "events": events
        });
        Request::builder()
            .method("POST")
            .uri("/api/sync")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap()
    };

    // First sync: events 1-3
    let resp = app(pool.clone())
        .oneshot(make_req(serde_json::json!([
            {"id": 1, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:00:00Z"},
            {"id": 2, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:01:00Z"},
            {"id": 3, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:02:00Z"},
        ])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Second sync: overlapping events 2-5; cursor is now 3 so only 4 and 5 should be accepted
    let resp = app(pool.clone())
        .oneshot(make_req(serde_json::json!([
            {"id": 2, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:01:00Z"},
            {"id": 3, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:02:00Z"},
            {"id": 4, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:03:00Z"},
            {"id": 5, "filter_name": null, "filter_hash": null,
             "input_tokens": 100, "output_tokens": 20, "command_count": 1,
             "recorded_at": "2026-01-01T00:04:00Z"},
        ])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        result["accepted"], 2,
        "only events 4 and 5 should be accepted"
    );
    assert_eq!(result["cursor"], 5);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_null_filter_hash_does_not_create_filter_stats(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [{
            "id": 1,
            "filter_name": "git/push",
            "filter_hash": null,
            "input_tokens": 1000,
            "output_tokens": 200,
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

    let stats_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filter_stats")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        stats_count, 0,
        "NULL filter_hash must not create a filter_stats entry"
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn sync_updates_filter_stats_for_known_hash(pool: PgPool) {
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine_id = create_machine(&pool, user_id).await;
    let hash = "abcdef1234567890";
    let body = serde_json::json!({
        "machine_id": machine_id.to_string(),
        "last_event_id": 0,
        "events": [
            {
                "id": 1,
                "filter_name": "git/push",
                "filter_hash": hash,
                "input_tokens": 1000,
                "output_tokens": 200,
                "command_count": 1,
                "recorded_at": "2026-01-01T00:00:00Z"
            },
            {
                "id": 2,
                "filter_name": "git/push",
                "filter_hash": hash,
                "input_tokens": 500,
                "output_tokens": 100,
                "command_count": 1,
                "recorded_at": "2026-01-01T00:01:00Z"
            }
        ]
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

    let (total_commands, total_input, total_output, savings_pct): (i64, i64, i64, f64) =
        sqlx::query_as(
            "SELECT total_commands, total_input_tokens, total_output_tokens, savings_pct
             FROM filter_stats WHERE filter_hash = $1",
        )
        .bind(hash)
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(total_commands, 2, "total_commands should be 2");
    assert_eq!(total_input, 1500, "total_input_tokens should be 1500");
    assert_eq!(total_output, 300, "total_output_tokens should be 300");
    // savings_pct = (1500 - 300) / 1500 = 0.8
    assert!(
        (savings_pct - 0.8).abs() < 0.001,
        "savings_pct should be ~0.8, got {savings_pct}"
    );
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
        publish_rate_limiter: Arc::new(PublishRateLimiter::new(100, 3600)),
        sync_rate_limiter: Arc::new(SyncRateLimiter::new(1, 3600)),
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

    // Second request: rate check fails (limit=1 already consumed)
    let resp2 = app.oneshot(make_req()).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::TOO_MANY_REQUESTS);
}
