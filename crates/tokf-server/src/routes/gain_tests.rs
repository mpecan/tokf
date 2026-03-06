use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::routes::test_helpers::*;

use super::{get_filter_gain, get_gain, get_global_gain};

fn app(pool: PgPool) -> Router {
    Router::new()
        .route("/api/gain", get(get_gain))
        .route("/api/gain/global", get(get_global_gain))
        .route("/api/gain/filter/{hash}", get(get_filter_gain))
        .with_state(make_state(pool))
}

async fn insert_usage_event(
    pool: &PgPool,
    machine_id: uuid::Uuid,
    input_tokens: i64,
    output_tokens: i64,
) {
    insert_usage_event_with_raw(pool, machine_id, input_tokens, output_tokens, input_tokens).await;
}

async fn insert_usage_event_with_raw(
    pool: &PgPool,
    machine_id: uuid::Uuid,
    input_tokens: i64,
    output_tokens: i64,
    raw_tokens: i64,
) {
    sqlx::query(
        "INSERT INTO usage_events (machine_id, input_tokens, output_tokens, command_count, recorded_at, raw_tokens)
         VALUES ($1, $2, $3, 1, NOW(), $4)",
    )
    .bind(machine_id)
    .bind(input_tokens)
    .bind(output_tokens)
    .bind(raw_tokens)
    .execute(pool)
    .await
    .unwrap();
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_gain_requires_auth(pool: PgPool) {
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_gain_returns_user_events_only(pool: PgPool) {
    init_test_tracing();
    let (user_id, token) = create_user_and_token(&pool).await;
    let (other_user_id, _) = create_user_and_token(&pool).await;
    let machine = create_machine(&pool, user_id).await;
    let other_machine = create_machine(&pool, other_user_id).await;
    insert_usage_event(&pool, machine, 1000, 200).await;
    insert_usage_event(&pool, other_machine, 9000, 1000).await;

    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["total_input_tokens"], 1000);
    assert_eq!(result["total_output_tokens"], 200);
    assert_eq!(result["total_raw_tokens"], 1000);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn global_gain_returns_all_events(pool: PgPool) {
    init_test_tracing();
    let (user1_id, _) = create_user_and_token(&pool).await;
    let (user2_id, _) = create_user_and_token(&pool).await;
    let m1 = create_machine(&pool, user1_id).await;
    let m2 = create_machine(&pool, user2_id).await;
    insert_usage_event(&pool, m1, 1000, 200).await;
    insert_usage_event(&pool, m2, 2000, 300).await;

    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain/global")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["total_input_tokens"], 3000);
    assert_eq!(result["total_output_tokens"], 500);
    assert_eq!(result["total_raw_tokens"], 3000);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn filter_gain_returns_404_for_unknown(pool: PgPool) {
    init_test_tracing();
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain/filter/doesnotexist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_status(resp, StatusCode::NOT_FOUND).await;
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_gain_empty_database_returns_zeros(pool: PgPool) {
    init_test_tracing();
    let (_, token) = create_user_and_token(&pool).await;
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["total_input_tokens"], 0);
    assert_eq!(result["total_output_tokens"], 0);
    assert_eq!(result["total_commands"], 0);
    assert_eq!(result["total_raw_tokens"], 0);
    assert_eq!(result["by_machine"].as_array().unwrap().len(), 0);
    assert_eq!(result["by_filter"].as_array().unwrap().len(), 0);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn global_gain_empty_database_returns_zeros(pool: PgPool) {
    init_test_tracing();
    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain/global")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["total_input_tokens"], 0);
    assert_eq!(result["total_output_tokens"], 0);
    assert_eq!(result["total_commands"], 0);
    assert_eq!(result["total_raw_tokens"], 0);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn filter_gain_returns_stats_with_savings_pct(pool: PgPool) {
    init_test_tracing();
    let hash = "teststats0000000000000000000000000000000000000000000000000000";

    // Insert filter_stats directly with a known savings_pct on the 0-100 scale
    sqlx::query(
        "INSERT INTO filter_stats
            (filter_hash, total_commands, total_input_tokens, total_output_tokens, savings_pct, total_raw_tokens, last_updated)
         VALUES ($1, 5, 2000, 400, 80.0, 3000, NOW())",
    )
    .bind(hash)
    .execute(&pool)
    .await
    .unwrap();

    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/gain/filter/{hash}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["filter_hash"], hash);
    assert_eq!(result["total_commands"], 5);
    assert_eq!(result["total_input_tokens"], 2000);
    assert_eq!(result["total_output_tokens"], 400);
    assert_eq!(
        result["savings_pct"], 80.0,
        "savings_pct should be 80.0 (0-100 scale)"
    );
    assert_eq!(result["total_raw_tokens"], 3000);
    assert!(result["last_updated"].is_string());
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn global_gain_does_not_expose_hostnames(pool: PgPool) {
    init_test_tracing();
    let (user_id, _) = create_user_and_token(&pool).await;
    let machine = create_machine(&pool, user_id).await;
    insert_usage_event(&pool, machine, 500, 100).await;

    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain/global")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // by_machine entries must not contain a "hostname" field
    for entry in result["by_machine"].as_array().unwrap() {
        assert!(
            entry.get("hostname").is_none(),
            "global gain must not expose hostname: {entry}"
        );
    }
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn get_gain_raw_tokens_distinct_from_input(pool: PgPool) {
    init_test_tracing();
    let (user_id, token) = create_user_and_token(&pool).await;
    let machine = create_machine(&pool, user_id).await;
    // raw_tokens (1500) > input_tokens (1000) — baseline adjustment occurred
    insert_usage_event_with_raw(&pool, machine, 1000, 200, 1500).await;

    let resp = app(pool)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/gain")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = assert_status(resp, StatusCode::OK).await;
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["total_input_tokens"], 1000);
    assert_eq!(result["total_raw_tokens"], 1500);
}
