use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::routes::test_helpers::insert_service_token;
use crate::storage::StorageClient as _;
use crate::storage::mock::InMemoryStorageClient;

use super::test_helpers::{
    insert_test_user, make_state_with_storage, post_json, publish_filter_helper,
};

const VALID_FILTER_TOML: &[u8] = b"command = \"my-tool\"\n";

const URI: &str = "/api/filters/backfill-v1-hashes";

/// Publish a filter, then NULL out its `v1_hash` to simulate a row created
/// before this PR shipped.
async fn publish_then_null_v1(
    pool: &PgPool,
    storage: Arc<InMemoryStorageClient>,
    user_token: &str,
    toml: &[u8],
) -> String {
    let state = make_state_with_storage(pool.clone(), storage);
    let app = crate::routes::create_router(state);
    let hash = publish_filter_helper(app, user_token, toml, &[]).await;
    sqlx::query("UPDATE filters SET v1_hash = NULL WHERE content_hash = $1")
        .bind(&hash)
        .execute(pool)
        .await
        .unwrap();
    hash
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_populates_null_rows(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, alice) = insert_test_user(&pool, "alice_bf").await;
    let (_, bob) = insert_test_user(&pool, "bob_bf").await;
    let service_token = insert_service_token(&pool, "bf-test").await;

    let alice_toml: &[u8] = b"command = \"alice-tool\"\n";
    let bob_toml: &[u8] = b"command = \"bob-tool\"\n";

    let alice_hash = publish_then_null_v1(&pool, Arc::clone(&storage), &alice, alice_toml).await;
    let bob_hash = publish_then_null_v1(&pool, Arc::clone(&storage), &bob, bob_toml).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = post_json(
        app,
        &service_token,
        URI,
        &serde_json::json!({ "limit": 100 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["processed"], 2);
    assert_eq!(json["updated"], 2);
    assert!(json["failed"].as_array().unwrap().is_empty());

    let alice_v1: Option<String> =
        sqlx::query_scalar("SELECT v1_hash FROM filters WHERE content_hash = $1")
            .bind(&alice_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        alice_v1.unwrap(),
        tokf_common::canonical_v1::hash(std::str::from_utf8(alice_toml).unwrap()).unwrap()
    );

    let bob_v1: Option<String> =
        sqlx::query_scalar("SELECT v1_hash FROM filters WHERE content_hash = $1")
            .bind(&bob_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        bob_v1.unwrap(),
        tokf_common::canonical_v1::hash(std::str::from_utf8(bob_toml).unwrap()).unwrap()
    );
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_skips_already_populated(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, user) = insert_test_user(&pool, "alice_skip").await;
    let service_token = insert_service_token(&pool, "bf-skip").await;

    publish_then_null_v1(&pool, Arc::clone(&storage), &user, VALID_FILTER_TOML).await;

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = post_json(
        app,
        &service_token,
        URI,
        &serde_json::json!({ "limit": 10 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let first: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(first["processed"], 1);
    assert_eq!(first["updated"], 1);

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = post_json(
        app,
        &service_token,
        URI,
        &serde_json::json!({ "limit": 10 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let second: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        second["processed"], 0,
        "second invocation should find no candidates"
    );
}

/// Drive a backfill against a single corrupted-R2 row and assert that:
/// the call succeeds with `processed=1, updated=0`, the row appears in
/// `failed[]`, and the row's `v1_hash` remains NULL. Used by every
/// per-failure-mode test below.
async fn run_backfill_expecting_one_failure(
    pool: &PgPool,
    storage: Arc<InMemoryStorageClient>,
    service_token: &str,
    hash: &str,
) {
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = post_json(app, service_token, URI, &serde_json::json!({ "limit": 10 })).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["processed"], 1);
    assert_eq!(json["updated"], 0);
    let failed = json["failed"].as_array().unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["content_hash"], hash);

    let v1: Option<String> =
        sqlx::query_scalar("SELECT v1_hash FROM filters WHERE content_hash = $1")
            .bind(hash)
            .fetch_one(pool)
            .await
            .unwrap();
    assert!(
        v1.is_none(),
        "v1_hash must remain NULL when backfill failed"
    );
}

async fn r2_key_for(pool: &PgPool, hash: &str) -> String {
    sqlx::query_scalar("SELECT r2_key FROM filters WHERE content_hash = $1")
        .bind(hash)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_reports_failure_when_r2_object_missing(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, user) = insert_test_user(&pool, "alice_missing").await;
    let service_token = insert_service_token(&pool, "bf-missing").await;

    let hash = publish_then_null_v1(&pool, Arc::clone(&storage), &user, VALID_FILTER_TOML).await;
    storage
        .delete(&r2_key_for(&pool, &hash).await)
        .await
        .unwrap();

    run_backfill_expecting_one_failure(&pool, storage, &service_token, &hash).await;
}

/// Exercises both UTF-8 and TOML-parse failure branches in
/// `compute_and_store_v1` by writing bytes that are invalid as both.
#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_reports_failure_when_r2_object_unparseable(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, user) = insert_test_user(&pool, "alice_corrupt").await;
    let service_token = insert_service_token(&pool, "bf-corrupt").await;

    let hash = publish_then_null_v1(&pool, Arc::clone(&storage), &user, VALID_FILTER_TOML).await;
    storage
        .put(
            &r2_key_for(&pool, &hash).await,
            b"\xff\xfe not valid toml [[[".to_vec(),
        )
        .await
        .unwrap();

    run_backfill_expecting_one_failure(&pool, storage, &service_token, &hash).await;
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_requires_service_token(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let app = crate::routes::create_router(make_state_with_storage(pool, storage));

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/filters/backfill-v1-hashes")
                .header("content-type", "application/json")
                .body(Body::from(b"{}".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_rejects_invalid_bearer(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let app = crate::routes::create_router(make_state_with_storage(pool, storage));

    let resp = post_json(
        app,
        "not-a-real-token",
        URI,
        &serde_json::json!({ "limit": 10 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_respects_limit(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let (_, user) = insert_test_user(&pool, "alice_limit").await;
    let service_token = insert_service_token(&pool, "bf-limit").await;

    // Publish 5 distinct filters; null their v1_hash.
    for i in 0..5 {
        let toml = format!("command = \"tool-{i}\"\n");
        publish_then_null_v1(&pool, Arc::clone(&storage), &user, toml.as_bytes()).await;
    }

    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));
    let resp = post_json(app, &service_token, URI, &serde_json::json!({ "limit": 2 })).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["processed"], 2, "should respect requested limit");
    assert_eq!(json["updated"], 2);

    let still_null: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM filters WHERE v1_hash IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still_null, 3, "3 rows should still be unprocessed");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn backfill_v1_caps_limit_at_max(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let service_token = insert_service_token(&pool, "bf-cap").await;
    let app =
        crate::routes::create_router(make_state_with_storage(pool.clone(), Arc::clone(&storage)));

    // No filters seeded; just confirm the call succeeds with an out-of-range
    // limit (clamped, not rejected).
    let resp = post_json(
        app,
        &service_token,
        URI,
        &serde_json::json!({ "limit": 1_000_000 }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["processed"], 0);
}
