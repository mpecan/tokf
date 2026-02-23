//! DB integration tests using `#[sqlx::test]`.
//!
//! Each test is marked `#[ignore]` so that `cargo test --workspace` passes
//! without a running Postgres instance.  To run these tests locally, set
//! `DATABASE_URL` and pass `--include-ignored`:
//!
//! ```sh
//! DATABASE_URL=postgres://tokf:tokf@localhost:5432/tokf_dev \
//!     cargo test -p tokf-server -- --include-ignored
//! ```
//!
//! In CI, the workflow runs `cargo test --workspace -- --include-ignored`
//! with a live postgres service, so all tests execute there.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tokf_server::{routes::create_router, state::AppState};
use tower::ServiceExt;

#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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

#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
async fn health_returns_200_and_ok_status_with_real_db(pool: PgPool) {
    let state = AppState { db: pool };
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

#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
async fn ready_returns_200_and_ok_status_with_real_db(pool: PgPool) {
    let state = AppState { db: pool };
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

#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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

#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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
#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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
#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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
#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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
#[ignore = "requires DATABASE_URL to be set"]
#[sqlx::test(migrations = "./migrations")]
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
