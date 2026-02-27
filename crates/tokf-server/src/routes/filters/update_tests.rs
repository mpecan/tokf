use axum::{
    Json,
    extract::{Path, State},
};
use bytes::Bytes;
use serde::Serialize;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::storage;

use super::publish::{MAX_TOTAL_SIZE, upload_tests};

// ── Response type ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UpdateTestsResponse {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub test_count: usize,
    pub registry_url: String,
}

// ── Multipart parsing (test files only) ─────────────────────────────────────

async fn parse_test_multipart(
    multipart: &mut axum::extract::Multipart,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let mut test_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_size: usize = 0;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(name) = field.name() else {
            continue;
        };
        let name = name.to_string();
        let bytes: Bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        total_size = total_size.saturating_add(bytes.len());
        if total_size > MAX_TOTAL_SIZE {
            return Err(AppError::BadRequest(
                "upload exceeds 1 MB total size limit".to_string(),
            ));
        }
        if let Some(filename) = name.strip_prefix("test:")
            && !filename.is_empty()
        {
            test_files.push((filename.to_string(), bytes.to_vec()));
        } else {
            tracing::debug!("ignoring unexpected multipart field: {name}");
        }
    }

    Ok(test_files)
}

/// Parse multipart, require at least one test file, and validate each.
async fn parse_and_validate_tests(
    multipart: &mut axum::extract::Multipart,
) -> Result<Vec<(String, Vec<u8>)>, AppError> {
    let test_files = parse_test_multipart(multipart).await?;
    if test_files.is_empty() {
        return Err(AppError::BadRequest(
            "at least one test file is required".to_string(),
        ));
    }
    for (filename, bytes) in &test_files {
        tokf_common::test_case::validate(bytes)
            .map_err(|e| AppError::BadRequest(format!("{filename}: {e}")))?;
    }
    Ok(test_files)
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn validate_hash(hash: &str) -> Result<(), AppError> {
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::BadRequest(
            "invalid content hash: expected 64 hex characters".to_string(),
        ));
    }
    Ok(())
}

/// Verify the filter exists, the caller is the author, and return the
/// `command_pattern` for the response.
async fn verify_author(state: &AppState, hash: &str, user_id: i64) -> Result<String, AppError> {
    let row = sqlx::query_as::<_, (i64, String)>(
        "SELECT f.author_id, f.command_pattern FROM filters f WHERE f.content_hash = $1",
    )
    .bind(hash)
    .fetch_optional(&state.db)
    .await?;

    let (author_id, command_pattern) =
        row.ok_or_else(|| AppError::NotFound(format!("filter not found: {hash}")))?;

    if author_id != user_id {
        return Err(AppError::Forbidden(
            "you are not the author of this filter".to_string(),
        ));
    }
    Ok(command_pattern)
}

/// Atomically swap test files in the DB: lock the filter row, delete old
/// `filter_tests` rows, insert new ones, and return the old R2 keys for
/// post-commit cleanup.
async fn swap_test_rows(
    state: &AppState,
    hash: &str,
    new_r2_keys: &[String],
) -> Result<Vec<String>, AppError> {
    let mut tx = state.db.begin().await?;

    sqlx::query("SELECT 1 FROM filters WHERE content_hash = $1 FOR UPDATE")
        .bind(hash)
        .fetch_optional(&mut *tx)
        .await?;

    let old_keys: Vec<String> =
        sqlx::query_scalar("SELECT r2_key FROM filter_tests WHERE filter_hash = $1")
            .bind(hash)
            .fetch_all(&mut *tx)
            .await?;

    sqlx::query("DELETE FROM filter_tests WHERE filter_hash = $1")
        .bind(hash)
        .execute(&mut *tx)
        .await?;

    for key in new_r2_keys {
        sqlx::query("INSERT INTO filter_tests (filter_hash, r2_key) VALUES ($1, $2)")
            .bind(hash)
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(old_keys)
}

// ── PUT /api/filters/{hash}/tests ────────────────────────────────────────────

/// Replace the test suite for a published filter.
///
/// Only the original author can update tests. The filter TOML itself is
/// immutable — identical content hash means identical filter identity.
///
/// Accepts a multipart form with `test:<filename>` fields only (total ≤ 1 MB).
///
/// # Errors
///
/// - `400 Bad Request` if no test files are provided, validation fails, or
///   size limits are exceeded.
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `403 Forbidden` if the caller is not the filter's author.
/// - `404 Not Found` if no filter with the given hash exists.
/// - `429 Too Many Requests` if the user exceeds rate limits.
/// - `500 Internal Server Error` on storage or database failures.
pub async fn update_tests(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<UpdateTestsResponse>, AppError> {
    validate_hash(&hash)?;

    if !state.publish_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }

    let command_pattern = verify_author(&state, &hash, auth.user_id).await?;

    let test_files = parse_and_validate_tests(&mut multipart).await?;

    // Upload new test files to storage BEFORE touching the DB.
    // If the transaction below fails, these become orphans — harmless and
    // overwritten on retry (storage puts are idempotent by key).
    let test_count = test_files.len();
    let new_r2_keys = upload_tests(&state, &hash, test_files).await?;

    // Atomically swap test rows; returns the old R2 keys for cleanup.
    let old_r2_keys = swap_test_rows(&state, &hash, &new_r2_keys).await?;

    // Best-effort cleanup: delete old storage objects AFTER the DB commit
    // succeeds. Failures here leave orphaned blobs but never corrupt state.
    if let Err(e) = storage::delete_tests_for_hash(&*state.storage, &old_r2_keys).await {
        tracing::warn!("failed to delete old test files from storage: {e}");
    }

    let registry_url = format!("{}/filters/{}", state.public_url, hash);
    Ok(Json(UpdateTestsResponse {
        content_hash: hash,
        command_pattern,
        author: auth.username,
        test_count,
        registry_url,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::storage::StorageClient;
    use crate::storage::mock::InMemoryStorageClient;

    use super::super::test_helpers::{
        insert_test_user, make_multipart, make_state_with_storage, publish_filter_helper, put_tests,
    };

    const VALID_FILTER_TOML: &[u8] = b"command = \"my-tool\"\n";

    fn valid_test(name: &str) -> Vec<u8> {
        format!("name = \"{name}\"\n\n[[expect]]\ncontains = \"hello\"\n").into_bytes()
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn author_can_update_tests(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "author_update").await;

        // Publish with 1 test file
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(
            app,
            &token,
            VALID_FILTER_TOML,
            &[("test:old.toml", &valid_test("old"))],
        )
        .await;

        // Verify 1 test in DB
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
                .bind(&hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "expected 1 test before update");

        // Update with 2 new test files
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[
                ("test:new1.toml", &valid_test("new1")),
                ("test:new2.toml", &valid_test("new2")),
            ],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["test_count"], 2);

        // Verify DB now has 2 rows
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
                .bind(&hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2, "expected 2 tests after update");

        // Verify old test file was deleted from storage
        assert!(
            !storage
                .exists(&format!("filters/{hash}/tests/old.toml"))
                .await
                .unwrap(),
            "old test file should be deleted from storage"
        );
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn non_author_gets_403(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, alice_token) = insert_test_user(&pool, "alice_forbidden").await;
        let (_, bob_token) = insert_test_user(&pool, "bob_forbidden").await;

        // Alice publishes
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &alice_token, VALID_FILTER_TOML, &[]).await;

        // Bob tries to update tests
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &bob_token,
            &hash,
            &[("test:new.toml", &valid_test("new"))],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn unknown_hash_gets_404(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_404").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let fake_hash = "0".repeat(64);
        let resp = put_tests(
            app,
            &token,
            &fake_hash,
            &[("test:new.toml", &valid_test("new"))],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn empty_test_upload_gets_400(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_empty").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        // PUT with no test files
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(app, &token, &hash, &[]).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("at least one test file"),
            "expected empty test error, got: {}",
            json["error"]
        );
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn invalid_test_toml_gets_400(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_invalid_toml").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        // PUT with malformed TOML
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[("test:bad.toml", b"not valid toml [[[")],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn auth_required(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_auth_req").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        // PUT without auth header
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let (body, content_type) = make_multipart(&[("test:basic.toml", &valid_test("basic"))]);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/filters/{hash}/tests"))
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn returns_updated_filter_details(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_details").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[
                ("test:a.toml", &valid_test("a")),
                ("test:b.toml", &valid_test("b")),
                ("test:c.toml", &valid_test("c")),
            ],
        )
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["content_hash"], hash);
        assert_eq!(json["command_pattern"], "my-tool");
        assert_eq!(json["author"], "user_details");
        assert_eq!(json["test_count"], 3);
        assert!(
            json["registry_url"].as_str().unwrap().contains(&hash),
            "registry_url should contain hash"
        );
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn invalid_hash_format_gets_400(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_bad_hash").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        // Too short
        let resp = put_tests(
            app,
            &token,
            "abc123",
            &[("test:new.toml", &valid_test("new"))],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        // Non-hex characters
        let resp = put_tests(
            app,
            &token,
            &format!("{}zzzz", "0".repeat(60)),
            &[("test:new.toml", &valid_test("new"))],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn semantically_invalid_test_gets_400(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_semantic").await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        // Valid TOML but missing [[expect]] block
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[("test:no_expect.toml", b"name = \"no expects\"")],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn zero_to_n_tests_update(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_zero_n").await;

        // Publish with no tests
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, VALID_FILTER_TOML, &[]).await;

        // Verify 0 tests
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
                .bind(&hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "expected 0 tests before update");

        // Update to 2 tests
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[
                ("test:a.toml", &valid_test("a")),
                ("test:b.toml", &valid_test("b")),
            ],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
                .bind(&hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2, "expected 2 tests after update");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn double_update_replaces_fully(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_double").await;

        // Publish with 1 test
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(
            app,
            &token,
            VALID_FILTER_TOML,
            &[("test:v1.toml", &valid_test("v1"))],
        )
        .await;

        // First update: 2 tests
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(
            app,
            &token,
            &hash,
            &[
                ("test:v2a.toml", &valid_test("v2a")),
                ("test:v2b.toml", &valid_test("v2b")),
            ],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Second update: 1 test
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(app, &token, &hash, &[("test:v3.toml", &valid_test("v3"))]).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify only 1 test remains
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM filter_tests WHERE filter_hash = $1")
                .bind(&hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "expected 1 test after second update");

        // Verify v2 tests were cleaned from storage
        assert!(
            !storage
                .exists(&format!("filters/{hash}/tests/v2a.toml"))
                .await
                .unwrap(),
            "v2a should be deleted from storage"
        );
        assert!(
            !storage
                .exists(&format!("filters/{hash}/tests/v2b.toml"))
                .await
                .unwrap(),
            "v2b should be deleted from storage"
        );
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn old_storage_objects_cleaned_up(pool: PgPool) {
        let storage = Arc::new(InMemoryStorageClient::new());
        let (_, token) = insert_test_user(&pool, "user_cleanup").await;

        // Publish with 2 tests
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(
            app,
            &token,
            VALID_FILTER_TOML,
            &[
                ("test:old1.toml", &valid_test("old1")),
                ("test:old2.toml", &valid_test("old2")),
            ],
        )
        .await;

        let initial_delete_count = storage.delete_count();

        // Update to 1 test
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = put_tests(app, &token, &hash, &[("test:new.toml", &valid_test("new"))]).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // 2 old tests should have been deleted from storage
        assert_eq!(
            storage.delete_count() - initial_delete_count,
            2,
            "expected 2 delete calls for old test files"
        );
    }
}
