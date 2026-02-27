use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use sqlx::Row as _;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

const fn default_limit() -> i64 {
    20
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FilterSummary {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub savings_pct: f64,
    pub total_commands: i64,
    /// ISO 8601 timestamp when the filter was first published. P3.2.
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct FilterDetails {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub savings_pct: f64,
    pub total_commands: i64,
    pub created_at: String,
    pub test_count: i64,
    pub registry_url: String,
}

#[derive(Debug, Serialize)]
pub struct TestFilePayload {
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct DownloadPayload {
    pub filter_toml: String,
    pub test_files: Vec<TestFilePayload>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Cap limit between 1 and 100 inclusive.
fn clamp_limit(limit: i64) -> i64 {
    limit.clamp(1, 100)
}

/// Extract the filename from an R2 key (`filters/{hash}/tests/{filename}`).
fn filename_from_r2_key(r2_key: &str) -> String {
    r2_key.rsplit('/').next().unwrap_or(r2_key).to_string()
}

/// Escape `\`, `%`, and `_` for use in a SQL ILIKE pattern.
///
/// Without escaping, user-supplied `%` or `_` characters would act as ILIKE
/// wildcards, matching any character sequence or any single character
/// respectively. Backslashes must be escaped first because the query uses
/// `ESCAPE '\\'` — an unescaped `\` would modify the interpretation of the
/// next character and produce unexpected matches.
fn escape_ilike(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// ── GET /api/filters ──────────────────────────────────────────────────────────

/// Search the community filter registry.
///
/// Returns filters sorted by a relevance score combining savings percentage
/// and usage volume. Requires a valid bearer token.
///
/// # Errors
///
/// - `400 Bad Request` if the query string exceeds 200 characters.
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `429 Too Many Requests` if the caller exceeds the search rate limit.
/// - `500 Internal Server Error` on database failures.
pub async fn search_filters(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<FilterSummary>>, AppError> {
    // P1.3: Reject unreasonably long queries to prevent DB performance issues.
    if params.q.len() > 200 {
        return Err(AppError::BadRequest(
            "search query must not exceed 200 characters".to_string(),
        ));
    }

    // P2.7: Rate limit search endpoint.
    if !state.search_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }

    let limit = clamp_limit(params.limit);
    // P1.1: Escape ILIKE wildcards in user-supplied query to prevent wildcard injection.
    let pattern = if params.q.is_empty() {
        "%".to_string()
    } else {
        format!("%{}%", escape_ilike(&params.q))
    };

    let rows = sqlx::query(
        "SELECT f.content_hash, f.command_pattern, u.username AS author,
                COALESCE(fs.savings_pct, 0.0) AS savings_pct,
                COALESCE(fs.total_commands, 0) AS total_commands,
                f.created_at::TEXT AS created_at
         FROM filters f
         JOIN users u ON u.id = f.author_id
         LEFT JOIN filter_stats fs ON fs.filter_hash = f.content_hash
         WHERE f.command_pattern ILIKE $1 ESCAPE '\\'
         ORDER BY COALESCE(fs.savings_pct, 0.0)
                  * (1.0 + LN(CAST(COALESCE(fs.total_commands, 0) + 1 AS FLOAT8))) DESC,
                  f.created_at DESC
         LIMIT $2",
    )
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    // Propagate DB mapping errors for all columns — COALESCE ensures they are
    // non-null so unwrap_or would only hide real schema/type mismatches.
    let summaries: Vec<FilterSummary> = rows
        .iter()
        .map(|row| -> Result<FilterSummary, sqlx::Error> {
            Ok(FilterSummary {
                content_hash: row.try_get("content_hash")?,
                command_pattern: row.try_get("command_pattern")?,
                author: row.try_get("author")?,
                savings_pct: row.try_get("savings_pct")?,
                total_commands: row.try_get("total_commands")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Internal(format!("db mapping error: {e}")))?;

    Ok(Json(summaries))
}

// ── GET /api/filters/:hash ────────────────────────────────────────────────────

/// Get details for a specific filter by content hash.
///
/// # Errors
///
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `404 Not Found` if no filter with the given hash exists.
/// - `500 Internal Server Error` on database failures.
pub async fn get_filter(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<FilterDetails>, AppError> {
    if !state.search_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }
    let row = sqlx::query(
        "SELECT f.content_hash, f.command_pattern, u.username AS author,
                COALESCE(fs.savings_pct, 0.0) AS savings_pct,
                COALESCE(fs.total_commands, 0) AS total_commands,
                f.created_at::TEXT AS created_at,
                (SELECT COUNT(*)::BIGINT FROM filter_tests
                 WHERE filter_hash = f.content_hash) AS test_count
         FROM filters f
         JOIN users u ON u.id = f.author_id
         LEFT JOIN filter_stats fs ON fs.filter_hash = f.content_hash
         WHERE f.content_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("filter not found: {hash}")))?;

    // Propagate DB mapping errors for all columns — COALESCE/casts ensure they
    // are non-null so unwrap_or would only hide real schema/type mismatches.
    let details = (|| -> Result<FilterDetails, sqlx::Error> {
        let content_hash: String = row.try_get("content_hash")?;
        let registry_url = format!("{}/filters/{}", state.public_url, content_hash);
        Ok(FilterDetails {
            content_hash,
            command_pattern: row.try_get("command_pattern")?,
            author: row.try_get("author")?,
            savings_pct: row.try_get("savings_pct")?,
            total_commands: row.try_get("total_commands")?,
            created_at: row.try_get("created_at")?,
            test_count: row.try_get("test_count")?,
            registry_url,
        })
    })()
    .map_err(|e| AppError::Internal(format!("db mapping error: {e}")))?;

    Ok(Json(details))
}

// ── GET /api/filters/:hash/download ──────────────────────────────────────────

/// Download a filter's TOML and test files by content hash.
///
/// # Errors
///
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `404 Not Found` if no filter with the given hash exists.
/// - `500 Internal Server Error` on storage or database failures.
pub async fn download_filter(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<DownloadPayload>, AppError> {
    if !state.search_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }
    let r2_key: Option<String> =
        sqlx::query_scalar("SELECT r2_key FROM filters WHERE content_hash = $1")
            .bind(&hash)
            .fetch_optional(&state.db)
            .await?;

    let r2_key = r2_key.ok_or_else(|| AppError::NotFound(format!("filter not found: {hash}")))?;

    // P2.1: Log R2 key internally but return a generic message to the client.
    let filter_bytes = state
        .storage
        .get(&r2_key)
        .await
        .map_err(|e| {
            tracing::warn!("storage error retrieving filter {}: {e}", &hash);
            AppError::Internal("storage error retrieving filter".to_string())
        })?
        .ok_or_else(|| {
            tracing::warn!("filter TOML missing from storage for hash {}", &hash);
            AppError::Internal("filter data not found in storage".to_string())
        })?;

    let filter_toml = String::from_utf8(filter_bytes)
        .map_err(|_| AppError::Internal("filter TOML is not valid UTF-8".to_string()))?;

    let test_keys: Vec<String> =
        sqlx::query_scalar("SELECT r2_key FROM filter_tests WHERE filter_hash = $1")
            .bind(&hash)
            .fetch_all(&state.db)
            .await?;

    let mut test_files = Vec::with_capacity(test_keys.len());
    for key in &test_keys {
        let bytes = state
            .storage
            .get(key)
            .await
            .map_err(|e| {
                tracing::warn!(
                    "storage error retrieving test file for filter {}: {e}",
                    &hash
                );
                AppError::Internal("storage error retrieving test file".to_string())
            })?
            .ok_or_else(|| {
                tracing::warn!("test file missing from storage for filter {}", &hash);
                AppError::Internal("test file data not found in storage".to_string())
            })?;
        let content = String::from_utf8(bytes)
            .map_err(|_| AppError::Internal("test file is not valid UTF-8".to_string()))?;
        test_files.push(TestFilePayload {
            filename: filename_from_r2_key(key),
            content,
        });
    }

    Ok(Json(DownloadPayload {
        filter_toml,
        test_files,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::sync::Arc;

    use axum::http::StatusCode;
    use http_body_util::BodyExt;

    use crate::storage::mock::InMemoryStorageClient;

    use super::super::test_helpers::{
        get_request, insert_test_user, make_state, make_state_with_storage, publish_filter_helper,
    };
    use super::escape_ilike;

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_returns_empty_for_empty_registry(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "search_empty").await;
        let app = crate::routes::create_router(make_state(pool));

        let resp = get_request(app, &token, "/api/filters").await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!([]));
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_matches_command_pattern(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "search_match").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        publish_filter_helper(app, &token, b"command = \"git push\"\n", &[]).await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        publish_filter_helper(app, &token, b"command = \"cargo build\"\n", &[]).await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, "/api/filters?q=git").await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 1, "expected 1 result for q=git");
        assert_eq!(results[0]["command_pattern"], "git push");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_empty_q_returns_all(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "search_all").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        for toml in [
            b"command = \"git push\"\n".as_slice(),
            b"command = \"cargo check\"\n",
        ] {
            let app = crate::routes::create_router(make_state_with_storage(
                pool.clone(),
                Arc::clone(&storage),
            ));
            publish_filter_helper(app, &token, toml, &[]).await;
        }

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, "/api/filters").await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 2, "expected all 2 results");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn get_filter_returns_404_for_unknown_hash(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "get_404").await;
        let app = crate::routes::create_router(make_state(pool));

        let resp = get_request(
            app,
            &token,
            "/api/filters/deadbeef00000000000000000000000000000000000000000000000000000000",
        )
        .await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn get_filter_returns_details(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "get_details").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(
            app,
            &token,
            b"command = \"git push\"\n",
            &[(
                "test/basic.toml",
                b"name = \"basic\"\n\n[[expect]]\ncontains = \"ok\"\n",
            )],
        )
        .await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, &format!("/api/filters/{hash}")).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["content_hash"], hash);
        assert_eq!(json["command_pattern"], "git push");
        assert_eq!(json["author"], "get_details");
        assert_eq!(json["savings_pct"], 0.0);
        assert_eq!(json["total_commands"], 0);
        assert_eq!(json["test_count"], 1);
        assert!(
            json["registry_url"].as_str().unwrap().contains(&hash),
            "registry_url should contain hash"
        );
        assert!(
            json["created_at"].is_string(),
            "created_at should be a string"
        );
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn download_returns_toml_content(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "dl_toml").await;
        let storage = Arc::new(InMemoryStorageClient::new());
        let filter_toml = b"command = \"git push\"\n";

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(app, &token, filter_toml, &[]).await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, &format!("/api/filters/{hash}/download")).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["filter_toml"].as_str().unwrap(),
            std::str::from_utf8(filter_toml).unwrap()
        );
        assert_eq!(json["test_files"], serde_json::json!([]));
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn download_returns_test_files(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "dl_tests").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let hash = publish_filter_helper(
            app,
            &token,
            b"command = \"git push\"\n",
            &[
                (
                    "test/basic.toml",
                    b"name = \"basic\"\n\n[[expect]]\ncontains = \"ok\"\n",
                ),
                (
                    "test/edge.toml",
                    b"name = \"edge\"\n\n[[expect]]\ncontains = \"ok\"\n",
                ),
            ],
        )
        .await;

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, &format!("/api/filters/{hash}/download")).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let test_files = json["test_files"].as_array().unwrap();
        assert_eq!(test_files.len(), 2, "expected 2 test files");

        let filenames: std::collections::HashSet<&str> = test_files
            .iter()
            .map(|f| f["filename"].as_str().unwrap())
            .collect();
        assert!(filenames.contains("basic.toml"), "expected basic.toml");
        assert!(filenames.contains("edge.toml"), "expected edge.toml");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn download_returns_404_for_unknown_hash(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "dl_404").await;
        let app = crate::routes::create_router(make_state(pool));

        let resp = get_request(
            app,
            &token,
            "/api/filters/deadbeef00000000000000000000000000000000000000000000000000000000/download",
        )
        .await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_limit_is_respected(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "search_limit").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        for toml in [
            b"command = \"git push\"\n".as_slice(),
            b"command = \"git pull\"\n",
            b"command = \"git fetch\"\n",
        ] {
            let app = crate::routes::create_router(make_state_with_storage(
                pool.clone(),
                Arc::clone(&storage),
            ));
            publish_filter_helper(app, &token, toml, &[]).await;
        }

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, "/api/filters?limit=1").await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(results.len(), 1, "limit=1 should return at most 1 result");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_rejects_oversized_query(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "search_big_q").await;
        let app = crate::routes::create_router(make_state(pool));

        let long_query = "a".repeat(201);
        let resp = get_request(app, &token, &format!("/api/filters?q={long_query}")).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn escape_ilike_leaves_normal_text_unchanged() {
        assert_eq!(escape_ilike("git push"), "git push");
        assert_eq!(escape_ilike("cargo"), "cargo");
    }

    #[test]
    fn escape_ilike_escapes_wildcards() {
        assert_eq!(escape_ilike("100%"), r"100\%");
        assert_eq!(escape_ilike("git_push"), r"git\_push");
        assert_eq!(escape_ilike("%git_"), r"\%git\_");
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn search_wildcard_injection_does_not_return_all(pool: PgPool) {
        let (_, token) = insert_test_user(&pool, "wildcard_test").await;
        let storage = Arc::new(InMemoryStorageClient::new());

        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        publish_filter_helper(app, &token, b"command = \"cargo build\"\n", &[]).await;

        // Search for "_" — without escaping this would match any single character
        // and return all filters. With proper escaping it matches literal underscore.
        let app = crate::routes::create_router(make_state_with_storage(
            pool.clone(),
            Arc::clone(&storage),
        ));
        let resp = get_request(app, &token, "/api/filters?q=_").await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let results: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            results.len(),
            0,
            "escaped underscore should match nothing (no underscore in 'cargo build')"
        );
    }
}
