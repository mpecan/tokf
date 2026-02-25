//! DB integration tests using `#[crdb_test]`.
//!
//! Each test is marked `#[ignore]` so that `cargo test --workspace` passes
//! without a running database.  To run these tests locally, start `CockroachDB`
//! via docker-compose and set `DATABASE_URL`:
//!
//! ```sh
//! # Start `CockroachDB` (from crates/tokf-server/)
//! docker compose up -d
//!
//! # Create the dev database
//! cockroach sql --insecure -e "CREATE DATABASE IF NOT EXISTS tokf_dev"
//!
//! DATABASE_URL=postgresql://root@localhost:26257/tokf_dev?sslmode=disable \
//!     cargo test -p tokf-server -- --include-ignored
//! ```
//!
//! In CI, the workflow runs `cargo test -p tokf-server -- --ignored`
//! with a live `CockroachDB` service, so all tests execute there.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tokf_server::{
    auth::{
        github::{AccessTokenResponse, DeviceCodeResponse, GitHubClient, GitHubOrg, GitHubUser},
        mock::NoOpGitHubClient,
        token::{AuthUser, generate_token, hash_token},
    },
    routes::create_router,
    state::AppState,
    storage::noop::NoOpStorageClient,
};
use tower::ServiceExt;

/// Handler that returns user info — used by `AuthUser` extractor tests.
async fn protected_user_info(user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "user_id": user.user_id,
        "username": user.username,
    }))
}

/// Handler that requires auth but returns a fixed response.
async fn protected_ok(_user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

fn db_state(pool: PgPool) -> AppState {
    AppState {
        db: pool,
        github: Arc::new(NoOpGitHubClient),
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: true,
    }
}

fn db_state_with_github(pool: PgPool, github: Arc<dyn GitHubClient>) -> AppState {
    AppState {
        db: pool,
        github,
        storage: Arc::new(NoOpStorageClient),
        github_client_id: "test-client-id".to_string(),
        github_client_secret: "test-client-secret".to_string(),
        trust_proxy: true,
    }
}

// ── Existing schema tests ───────────────────────────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn migrations_apply_cleanly_and_all_tables_exist(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public'
         ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .expect("failed to query tables");

    let expected = [
        "auth_tokens",
        "device_flows",
        "filter_stats",
        "filter_tests",
        "filters",
        "machines",
        "sync_cursors",
        "usage_events",
        "users",
    ];
    for name in &expected {
        assert!(
            tables.iter().any(|t| t == name),
            "missing table: {name}, found: {tables:?}"
        );
    }
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn health_returns_200_and_ok_status_with_real_db(pool: PgPool) {
    let state = db_state(pool);
    let app = create_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("failed to build request"),
        )
        .await
        .expect("failed to get response");

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("invalid JSON");
    assert_eq!(json["status"], "ok");
    assert!(json["version"].is_string());
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn ready_returns_200_and_ok_status_with_real_db(pool: PgPool) {
    let state = db_state(pool);
    let app = create_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .expect("failed to build request"),
        )
        .await
        .expect("failed to get response");

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("invalid JSON");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["database"], "ok");
    assert!(json["version"].is_string());
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn users_unique_constraint_enforced(pool: PgPool) {
    sqlx::query(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (1, 'alice', 'https://example.com/avatar.png', 'https://github.com/alice')",
    )
    .execute(&pool)
    .await
    .expect("first insert should succeed");

    let result = sqlx::query(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (1, 'alice2', 'https://example.com/avatar2.png', 'https://github.com/alice2')",
    )
    .execute(&pool)
    .await;

    assert!(result.is_err(), "duplicate github_id should be rejected");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn machines_uuid_pk_auto_generated(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (42, 'bob', 'https://example.com/bob.png', 'https://github.com/bob')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("user insert failed");

    let machine_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO machines (user_id, hostname) VALUES ($1, 'dev-box') RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .expect("machine insert failed");

    assert_ne!(machine_id, uuid::Uuid::nil());
}

/// T-5: Verify column defaults are applied correctly on insert.
#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn users_column_defaults_are_applied(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (99, 'charlie', 'https://example.com/c.png', 'https://github.com/c')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("user insert failed");

    let visible: bool = sqlx::query_scalar("SELECT visible FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("select visible failed");
    assert!(visible, "visible should default to true");

    let orgs: serde_json::Value = sqlx::query_scalar("SELECT orgs FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("select orgs failed");
    assert_eq!(orgs, serde_json::json!([]), "orgs should default to []");

    // created_at and updated_at should be set automatically
    let created_at_is_set: bool =
        sqlx::query_scalar("SELECT created_at IS NOT NULL FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("select created_at failed");
    assert!(created_at_is_set, "created_at should be set by default");
}

/// T-2: Deleting a filter cascades to `filter_tests`, `usage_events`, and `filter_stats`.
#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn filter_delete_cascades_to_related_tables(pool: PgPool) {
    // Insert a user (required for filters FK)
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (10, 'dave', 'https://example.com/dave.png', 'https://github.com/dave')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("user insert failed");

    // Insert a machine (required for usage_events FK)
    let machine_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO machines (user_id, hostname) VALUES ($1, 'laptop') RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .expect("machine insert failed");

    // Insert a filter
    sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key)
         VALUES ('abc123', 'cargo*', 'cargo', $1, 'r2/abc123')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("filter insert failed");

    // Insert related rows
    sqlx::query("INSERT INTO filter_tests (filter_hash, r2_key) VALUES ('abc123', 'r2/test')")
        .execute(&pool)
        .await
        .expect("filter_test insert failed");

    sqlx::query("INSERT INTO usage_events (filter_hash, machine_id) VALUES ('abc123', $1)")
        .bind(machine_id)
        .execute(&pool)
        .await
        .expect("usage_event insert failed");

    sqlx::query("INSERT INTO filter_stats (filter_hash) VALUES ('abc123')")
        .execute(&pool)
        .await
        .expect("filter_stats insert failed");

    // Delete the filter — should cascade to all related tables
    sqlx::query("DELETE FROM filters WHERE content_hash = 'abc123'")
        .execute(&pool)
        .await
        .expect("filter delete failed");

    let test_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = 'abc123'")
            .fetch_one(&pool)
            .await
            .expect("count failed");
    assert_eq!(test_count, 0, "filter_tests should be deleted via cascade");

    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_events WHERE filter_hash = 'abc123'")
            .fetch_one(&pool)
            .await
            .expect("count failed");
    assert_eq!(event_count, 0, "usage_events should be deleted via cascade");

    let stats_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM filter_stats WHERE filter_hash = 'abc123'")
            .fetch_one(&pool)
            .await
            .expect("count failed");
    assert_eq!(stats_count, 0, "filter_stats should be deleted via cascade");
}

/// T-2: Inserting a `filter_test` with a non-existent `filter_hash` should fail.
#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn filter_test_orphan_insert_fails(pool: PgPool) {
    let result = sqlx::query(
        "INSERT INTO filter_tests (filter_hash, r2_key)
         VALUES ('nonexistent-hash', 'r2/test')",
    )
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "inserting filter_test with unknown filter_hash should fail"
    );
}

/// T-2: `usage_events` CHECK constraints reject negative values.
#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn usage_events_check_constraints_reject_negative_values(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (200, 'eve', 'https://example.com/eve.png', 'https://github.com/eve')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("user insert failed");

    let machine_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO machines (user_id, hostname) VALUES ($1, 'pc') RETURNING id",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .expect("machine insert failed");

    sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key)
         VALUES ('xyz', 'go*', 'go', $1, 'r2/xyz')",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("filter insert failed");

    let result = sqlx::query(
        "INSERT INTO usage_events (filter_hash, machine_id, input_tokens)
         VALUES ('xyz', $1, -1)",
    )
    .bind(machine_id)
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "negative input_tokens should be rejected by CHECK constraint"
    );
}

// ── Device flow DB tests ────────────────────────────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn device_flow_table_exists_after_migration(pool: PgPool) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name = 'device_flows'",
    )
    .fetch_all(&pool)
    .await
    .expect("failed to query tables");

    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0], "device_flows");
}

/// Mock GitHub client that returns a successful access token and user profile.
struct SuccessGitHubClient;

#[async_trait::async_trait]
impl GitHubClient for SuccessGitHubClient {
    async fn request_device_code(&self, _client_id: &str) -> anyhow::Result<DeviceCodeResponse> {
        Ok(DeviceCodeResponse {
            device_code: format!("dc-{}", rand::random::<u32>()),
            user_code: "TEST-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        })
    }

    async fn poll_access_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _device_code: &str,
    ) -> anyhow::Result<AccessTokenResponse> {
        Ok(AccessTokenResponse::Success {
            access_token: "gho_test_token".to_string(),
            token_type: "bearer".to_string(),
            scope: "read:user,read:org".to_string(),
        })
    }

    async fn get_user(&self, _access_token: &str) -> anyhow::Result<GitHubUser> {
        Ok(GitHubUser {
            id: 12345,
            login: "testuser".to_string(),
            avatar_url: "https://avatars.githubusercontent.com/u/12345".to_string(),
            html_url: "https://github.com/testuser".to_string(),
        })
    }

    async fn get_user_orgs(&self, _access_token: &str) -> anyhow::Result<Vec<GitHubOrg>> {
        Ok(vec![GitHubOrg {
            login: "test-org".to_string(),
        }])
    }
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn full_device_flow_creates_user_and_token(pool: PgPool) {
    let state = db_state_with_github(pool.clone(), Arc::new(SuccessGitHubClient));
    let app = create_router(state);

    // Step 1: Initiate device flow
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let device_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let device_code = device_resp["device_code"].as_str().unwrap();

    // Step 2: Poll for token (mock returns success immediately)
    let poll_body = serde_json::json!({ "device_code": device_code });
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(poll_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let token_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert!(token_resp["access_token"].is_string());
    assert_eq!(token_resp["token_type"], "bearer");
    assert_eq!(token_resp["user"]["username"], "testuser");
    assert!(token_resp["user"]["id"].as_i64().unwrap() > 0); // CockroachDB BIGSERIAL uses unique_rowid(), not sequential 1,2,3

    // Verify user was created in DB
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE github_id = 12345")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user_count, 1);

    // Verify auth token was created
    let bearer = token_resp["access_token"].as_str().unwrap();
    let bearer_hash = hash_token(bearer);
    let token_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_tokens WHERE token_hash = $1")
            .bind(&bearer_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(token_count, 1);

    // Verify device flow was marked completed
    let flow_status: String =
        sqlx::query_scalar("SELECT status FROM device_flows WHERE device_code = $1")
            .bind(device_code)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(flow_status, "completed");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_token_with_unknown_device_code_returns_404(pool: PgPool) {
    let state = db_state(pool);
    let app = create_router(state);

    let body = serde_json::json!({ "device_code": "nonexistent-code" });
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn rate_limit_rejects_11th_device_flow(pool: PgPool) {
    // Insert 10 device flows for the same IP
    for i in 0..10 {
        sqlx::query(
            "INSERT INTO device_flows (device_code, user_code, verification_uri, ip_address, expires_at)
             VALUES ($1, $2, 'https://github.com/login/device', 'test-ip', NOW() + INTERVAL '15 minutes')",
        )
        .bind(format!("dc-{i}"))
        .bind(format!("CODE-{i}"))
        .execute(&pool)
        .await
        .unwrap();
    }

    let state = db_state(pool);
    let app = create_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .header("x-forwarded-for", "test-ip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn expired_flows_are_cleaned_up_on_initiate(pool: PgPool) {
    // Insert an expired flow
    sqlx::query(
        "INSERT INTO device_flows (device_code, user_code, verification_uri, ip_address, expires_at)
         VALUES ('expired-dc', 'EXP-1234', 'https://github.com/login/device', '1.2.3.4', NOW() - INTERVAL '1 hour')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let state = db_state(pool.clone());
    let app = create_router(state);

    // Initiate a new flow — should trigger cleanup
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify expired flow was cleaned up
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM device_flows WHERE device_code = 'expired-dc'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "expired flow should have been cleaned up");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_user_extractor_with_valid_token(pool: PgPool) {
    // Create a user and token directly in the DB
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (999, 'extractor-test', 'https://example.com/a.png', 'https://github.com/e')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let token = generate_token();
    let token_hash = hash_token(&token);

    sqlx::query(
        "INSERT INTO auth_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, NOW() + INTERVAL '1 day')",
    )
    .bind(user_id)
    .bind(&token_hash)
    .execute(&pool)
    .await
    .unwrap();

    let state = db_state(pool);
    let app = Router::new()
        .route("/protected", get(protected_user_info))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["user_id"], user_id);
    assert_eq!(json["username"], "extractor-test");
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_user_extractor_rejects_expired_token(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (998, 'expired-test', 'https://example.com/a.png', 'https://github.com/e')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let token = generate_token();
    let token_hash = hash_token(&token);

    sqlx::query(
        "INSERT INTO auth_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, NOW() - INTERVAL '1 day')",
    )
    .bind(user_id)
    .bind(&token_hash)
    .execute(&pool)
    .await
    .unwrap();

    let state = db_state(pool);
    let app = Router::new()
        .route("/protected", get(protected_ok))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_user_extractor_rejects_missing_header(pool: PgPool) {
    let state = db_state(pool);
    let app = Router::new()
        .route("/protected", get(protected_ok))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Completed re-poll / idempotency test ────────────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn completed_device_code_repoll_issues_new_token(pool: PgPool) {
    let state = db_state_with_github(pool.clone(), Arc::new(SuccessGitHubClient));
    let app = create_router(state);

    // Step 1: Initiate device flow
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let device_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let device_code = device_resp["device_code"].as_str().unwrap().to_string();

    // Step 2: First poll (success)
    let poll_body = serde_json::json!({ "device_code": &device_code });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(poll_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let first_token: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let first_access_token = first_token["access_token"].as_str().unwrap().to_string();

    // Step 3: Re-poll — should get a new token, not an error
    let poll_body = serde_json::json!({ "device_code": &device_code });
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(poll_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let second_token: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(second_token["access_token"].is_string());
    assert_ne!(
        second_token["access_token"].as_str().unwrap(),
        first_access_token,
        "re-poll should generate a new token"
    );
}

// ── NULL expires_at (never-expiring token) test ─────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn auth_user_extractor_accepts_null_expires_at(pool: PgPool) {
    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url)
         VALUES (997, 'null-expiry', 'https://example.com/a.png', 'https://github.com/n')
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let token = generate_token();
    let token_hash = hash_token(&token);

    // Insert token with NULL expires_at (never expires)
    sqlx::query(
        "INSERT INTO auth_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, NULL)",
    )
    .bind(user_id)
    .bind(&token_hash)
    .execute(&pool)
    .await
    .unwrap();

    let state = db_state(pool);
    let app = Router::new()
        .route("/protected", get(protected_user_info))
        .with_state(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["user_id"], user_id);
}

// ── Unknown GitHub error test ───────────────────────────────────────────────

/// Mock GitHub client that returns an unknown error during polling.
struct UnknownErrorGitHubClient;

#[async_trait::async_trait]
impl GitHubClient for UnknownErrorGitHubClient {
    async fn request_device_code(&self, _client_id: &str) -> anyhow::Result<DeviceCodeResponse> {
        Ok(DeviceCodeResponse {
            device_code: format!("dc-{}", rand::random::<u32>()),
            user_code: "ERR-1234".to_string(),
            verification_uri: "https://github.com/login/device".to_string(),
            expires_in: 900,
            interval: 5,
        })
    }

    async fn poll_access_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _device_code: &str,
    ) -> anyhow::Result<AccessTokenResponse> {
        Ok(AccessTokenResponse::Pending {
            error: "some_unknown_error".to_string(),
            error_description: Some("An unexpected error occurred".to_string()),
            interval: None,
        })
    }

    async fn get_user(&self, _access_token: &str) -> anyhow::Result<GitHubUser> {
        Ok(GitHubUser {
            id: 1,
            login: "mock".to_string(),
            avatar_url: "https://example.com/a.png".to_string(),
            html_url: "https://github.com/mock".to_string(),
        })
    }

    async fn get_user_orgs(&self, _access_token: &str) -> anyhow::Result<Vec<GitHubOrg>> {
        Ok(vec![])
    }
}

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn unknown_github_error_returns_500(pool: PgPool) {
    let state = db_state_with_github(pool.clone(), Arc::new(UnknownErrorGitHubClient));
    let app = create_router(state);

    // Initiate device flow
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let device_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let device_code = device_resp["device_code"].as_str().unwrap();

    // Poll — should get 500 for unknown error
    let poll_body = serde_json::json!({ "device_code": device_code });
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(poll_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ── Token response includes expires_in ──────────────────────────────────────

#[crdb_test_macro::crdb_test(migrations = "./migrations")]
async fn token_response_includes_expires_in(pool: PgPool) {
    let state = db_state_with_github(pool.clone(), Arc::new(SuccessGitHubClient));
    let app = create_router(state);

    // Initiate
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let device_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let device_code = device_resp["device_code"].as_str().unwrap();

    // Poll
    let poll_body = serde_json::json!({ "device_code": device_code });
    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/api/auth/token")
                .header("content-type", "application/json")
                .body(Body::from(poll_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["expires_in"], 7_776_000,
        "should be 90 days in seconds"
    );
}
