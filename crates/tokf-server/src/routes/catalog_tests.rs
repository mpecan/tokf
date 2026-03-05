use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

use crate::catalog::{CatalogEntry, CatalogIndex};
use crate::routes::test_helpers::{insert_service_token, make_state};
use crate::storage::StorageClient as _;
use crate::storage::mock::InMemoryStorageClient;

async fn post_refresh(app: axum::Router, token: &str) -> axum::response::Response {
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/catalog/refresh")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
}

fn make_state_with_storage(
    pool: PgPool,
    storage: Arc<InMemoryStorageClient>,
) -> crate::state::AppState {
    crate::state::AppState {
        storage,
        ..make_state(pool)
    }
}

/// Insert a test user, returning `user_id`. Accepts visibility flag.
async fn insert_user(pool: &PgPool, username: &str, visible: bool) -> i64 {
    let github_id = crate::routes::test_helpers::rand_i64();
    sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url, visible)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(github_id)
    .bind(username)
    .bind(format!("https://github.com/{username}.png"))
    .bind(format!("https://github.com/{username}"))
    .bind(visible)
    .fetch_one(pool)
    .await
    .expect("failed to insert user")
}

/// Insert a filter with the given `content_hash` for the given author.
async fn insert_filter(pool: &PgPool, hash: &str, command: &str, author_id: i64, is_stdlib: bool) {
    let canonical = command.split_whitespace().next().unwrap_or(command);
    sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key, is_stdlib)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(hash)
    .bind(command)
    .bind(canonical)
    .bind(author_id)
    .bind(format!("filters/{hash}/filter.toml"))
    .bind(is_stdlib)
    .execute(pool)
    .await
    .expect("failed to insert filter");
}

/// Insert a test record for a filter.
async fn insert_test(pool: &PgPool, hash: &str, filename: &str) {
    sqlx::query("INSERT INTO filter_tests (filter_hash, r2_key) VALUES ($1, $2)")
        .bind(hash)
        .bind(format!("filters/{hash}/tests/{filename}"))
        .execute(pool)
        .await
        .expect("failed to insert filter test");
}

/// Insert filter stats for a filter.
#[allow(clippy::cast_precision_loss)]
async fn insert_stats(pool: &PgPool, hash: &str, commands: i64, input: i64, output: i64) {
    let savings_pct = if input > 0 {
        (input - output) as f64 / input as f64 * 100.0
    } else {
        0.0
    };
    sqlx::query(
        "INSERT INTO filter_stats (filter_hash, total_commands, total_input_tokens, total_output_tokens, savings_pct)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(hash)
    .bind(commands)
    .bind(input)
    .bind(output)
    .bind(savings_pct)
    .execute(pool)
    .await
    .expect("failed to insert filter stats");
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_requires_service_token(pool: PgPool) {
    let state = make_state(pool);
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, "invalid-token").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_empty_catalog(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let token = insert_service_token(&pool, "test").await;
    let state = make_state_with_storage(pool, storage.clone());
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["filters_count"], 0);
    assert!(json["generated_at"].as_str().is_some());

    // Verify R2 has valid JSON
    let r2_bytes = storage.get("catalog/index.json").await.unwrap().unwrap();
    let index: CatalogIndex = serde_json::from_slice(&r2_bytes).unwrap();
    assert_eq!(index.version, 1);
    assert!(index.filters.is_empty());
    assert_eq!(index.global_stats.total_filters, 0);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_with_filters(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let token = insert_service_token(&pool, "test").await;
    let user_id = insert_user(&pool, "alice", true).await;
    insert_filter(&pool, "hash1", "git push", user_id, false).await;
    insert_filter(&pool, "hash2", "cargo test", user_id, true).await;
    insert_test(&pool, "hash1", "basic.toml").await;
    insert_test(&pool, "hash2", "basic.toml").await;
    insert_test(&pool, "hash2", "advanced.toml").await;

    let state = make_state_with_storage(pool, storage.clone());
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["filters_count"], 2);

    // Verify catalog/index.json
    let r2_bytes = storage.get("catalog/index.json").await.unwrap().unwrap();
    let index: CatalogIndex = serde_json::from_slice(&r2_bytes).unwrap();
    assert_eq!(index.filters.len(), 2);
    assert_eq!(index.global_stats.total_filters, 2);

    // Verify per-filter metadata files
    let meta1_bytes = storage
        .get("filters/hash1/metadata.json")
        .await
        .unwrap()
        .unwrap();
    let meta1: CatalogEntry = serde_json::from_slice(&meta1_bytes).unwrap();
    assert_eq!(meta1.content_hash, "hash1");
    assert_eq!(meta1.command_pattern, "git push");
    assert_eq!(meta1.test_count, 1);
    assert!(!meta1.is_stdlib);

    let meta2_bytes = storage
        .get("filters/hash2/metadata.json")
        .await
        .unwrap()
        .unwrap();
    let meta2: CatalogEntry = serde_json::from_slice(&meta2_bytes).unwrap();
    assert_eq!(meta2.content_hash, "hash2");
    assert_eq!(meta2.test_count, 2);
    assert!(meta2.is_stdlib);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_includes_stats(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let token = insert_service_token(&pool, "test").await;
    let user_id = insert_user(&pool, "bob", true).await;
    insert_filter(&pool, "hash_stats", "npm test", user_id, false).await;
    insert_stats(&pool, "hash_stats", 42, 10000, 4000).await;

    let state = make_state_with_storage(pool, storage.clone());
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let r2_bytes = storage.get("catalog/index.json").await.unwrap().unwrap();
    let index: CatalogIndex = serde_json::from_slice(&r2_bytes).unwrap();
    assert_eq!(index.filters.len(), 1);

    let entry = &index.filters[0];
    assert_eq!(entry.stats.total_commands, 42);
    assert_eq!(entry.stats.total_input_tokens, 10000);
    assert_eq!(entry.stats.total_output_tokens, 4000);
    assert!((entry.stats.savings_pct - 60.0).abs() < 0.1);

    // Global stats should match
    assert_eq!(index.global_stats.total_commands, 42);
    assert!((index.global_stats.overall_savings_pct - 60.0).abs() < 0.1);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_includes_author_details(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let token = insert_service_token(&pool, "test").await;
    let user_id = insert_user(&pool, "carol", true).await;
    insert_filter(&pool, "hash_author", "docker build", user_id, false).await;

    let state = make_state_with_storage(pool, storage.clone());
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let r2_bytes = storage
        .get("filters/hash_author/metadata.json")
        .await
        .unwrap()
        .unwrap();
    let entry: CatalogEntry = serde_json::from_slice(&r2_bytes).unwrap();
    assert_eq!(entry.author.username, "carol");
    assert_eq!(entry.author.avatar_url, "https://github.com/carol.png");
    assert_eq!(entry.author.profile_url, "https://github.com/carol");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn refresh_redacts_invisible_authors(pool: PgPool) {
    let storage = Arc::new(InMemoryStorageClient::new());
    let token = insert_service_token(&pool, "test").await;
    let ghost_id = insert_user(&pool, "ghost-dev", false).await;
    insert_filter(&pool, "hash_ghost", "go build", ghost_id, true).await;

    let state = make_state_with_storage(pool, storage.clone());
    let app = crate::routes::create_router(state);

    let resp = post_refresh(app, &token).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let r2_bytes = storage
        .get("filters/hash_ghost/metadata.json")
        .await
        .unwrap()
        .unwrap();
    let entry: CatalogEntry = serde_json::from_slice(&r2_bytes).unwrap();
    assert_eq!(
        entry.author.username, "tokf",
        "non-visible author should be redacted"
    );
    assert_eq!(
        entry.author.avatar_url, "",
        "non-visible avatar should be empty"
    );
    assert_eq!(
        entry.author.profile_url, "",
        "non-visible profile should be empty"
    );

    // Also check in the catalog index
    let index_bytes = storage.get("catalog/index.json").await.unwrap().unwrap();
    let index: CatalogIndex = serde_json::from_slice(&index_bytes).unwrap();
    let ghost_entry = &index.filters[0];
    assert_eq!(ghost_entry.author.username, "tokf");
}
