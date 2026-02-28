use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
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
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<(HeaderMap, Json<Vec<FilterSummary>>), AppError> {
    // P1.3: Reject unreasonably long queries to prevent DB performance issues.
    if params.q.len() > 200 {
        return Err(AppError::BadRequest(
            "search query must not exceed 200 characters".to_string(),
        ));
    }

    // Per-IP rate limit (60/min).
    let ip = crate::routes::ip::extract_ip(&headers, state.trust_proxy, None);
    let ip_rl = state.ip_search_rate_limiter.check_and_increment(ip);
    if !ip_rl.allowed {
        return Err(AppError::rate_limited(&ip_rl));
    }

    // Per-user rate limit.
    let user_rl = state.search_rate_limiter.check_and_increment(auth.user_id);
    if !user_rl.allowed {
        return Err(AppError::rate_limited(&user_rl));
    }

    let rl = crate::routes::ip::most_restrictive(ip_rl, user_rl);

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

    Ok((crate::routes::ip::rate_limit_headers(&rl), Json(summaries)))
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
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<(HeaderMap, Json<FilterDetails>), AppError> {
    let ip = crate::routes::ip::extract_ip(&headers, state.trust_proxy, None);
    let ip_rl = state.ip_search_rate_limiter.check_and_increment(ip);
    if !ip_rl.allowed {
        return Err(AppError::rate_limited(&ip_rl));
    }
    let user_rl = state.search_rate_limiter.check_and_increment(auth.user_id);
    if !user_rl.allowed {
        return Err(AppError::rate_limited(&user_rl));
    }
    let rl = crate::routes::ip::most_restrictive(ip_rl, user_rl);
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

    Ok((crate::routes::ip::rate_limit_headers(&rl), Json(details)))
}

// ── GET /api/filters/:hash/download ──────────────────────────────────────────

/// Download a filter's TOML and test files by content hash.
///
/// # Errors
///
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `404 Not Found` if no filter with the given hash exists.
/// - `500 Internal Server Error` on storage or database failures.
// 8 lines over the 60-line guideline due to per-IP + per-user rate-limit checks.
#[allow(clippy::too_many_lines)]
pub async fn download_filter(
    auth: AuthUser,
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<(HeaderMap, Json<DownloadPayload>), AppError> {
    let ip = crate::routes::ip::extract_ip(&headers, state.trust_proxy, None);
    let ip_rl = state.ip_download_rate_limiter.check_and_increment(ip);
    if !ip_rl.allowed {
        return Err(AppError::rate_limited(&ip_rl));
    }
    let user_rl = state.search_rate_limiter.check_and_increment(auth.user_id);
    if !user_rl.allowed {
        return Err(AppError::rate_limited(&user_rl));
    }
    let rl = crate::routes::ip::most_restrictive(ip_rl, user_rl);
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

    Ok((
        crate::routes::ip::rate_limit_headers(&rl),
        Json(DownloadPayload {
            filter_toml,
            test_files,
        }),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────
// DB integration tests live in `search_tests.rs` (same pattern as sync/sync_tests).

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::escape_ilike;

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
}
