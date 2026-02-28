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

/// Parsed and validated test upload.
struct ValidatedTests {
    files: Vec<(String, Vec<u8>)>,
    cases: Vec<tokf_common::test_case::TestCase>,
}

/// Parse multipart, require at least one test file, validate each, and return
/// both raw bytes (for storage) and parsed cases (for verification).
async fn parse_and_validate_tests(
    multipart: &mut axum::extract::Multipart,
) -> Result<ValidatedTests, AppError> {
    let test_files = parse_test_multipart(multipart).await?;
    if test_files.is_empty() {
        return Err(AppError::BadRequest(
            "at least one test file is required".to_string(),
        ));
    }
    let mut cases = Vec::with_capacity(test_files.len());
    for (filename, bytes) in &test_files {
        let tc = tokf_common::test_case::validate(bytes)
            .map_err(|e| AppError::BadRequest(format!("{filename}: {e}")))?;
        cases.push(tc);
    }
    Ok(ValidatedTests {
        files: test_files,
        cases,
    })
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Download the filter TOML from storage and parse it into a `FilterConfig`.
async fn load_filter_config(
    state: &AppState,
    hash: &str,
) -> Result<tokf_common::config::types::FilterConfig, AppError> {
    let key = format!("filters/{hash}/filter.toml");
    let bytes = state
        .storage
        .get(&key)
        .await
        .map_err(|e| AppError::Internal(format!("failed to download filter: {e}")))?
        .ok_or_else(|| AppError::Internal(format!("filter TOML missing from storage: {hash}")))?;
    let toml_str = std::str::from_utf8(&bytes)
        .map_err(|_| AppError::Internal("stored filter TOML is not valid UTF-8".to_string()))?;
    toml::from_str(toml_str)
        .map_err(|e| AppError::Internal(format!("stored filter TOML is invalid: {e}")))
}

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

    let locked = sqlx::query("SELECT 1 FROM filters WHERE content_hash = $1 FOR UPDATE")
        .bind(hash)
        .fetch_optional(&mut *tx)
        .await?;
    if locked.is_none() {
        return Err(AppError::NotFound(format!(
            "filter deleted during update: {hash}"
        )));
    }

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
// 10 lines over the 60-line guideline due to rate-limit checks and server-side
// test verification with timeout handling.
#[allow(clippy::too_many_lines)]
pub async fn update_tests(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
    mut multipart: axum::extract::Multipart,
) -> Result<(axum::http::HeaderMap, Json<UpdateTestsResponse>), AppError> {
    validate_hash(&hash)?;

    let rl = state.publish_rate_limiter.check_and_increment(auth.user_id);
    if !rl.allowed {
        return Err(AppError::rate_limited(&rl));
    }

    let command_pattern = verify_author(&state, &hash, auth.user_id).await?;

    let validated = parse_and_validate_tests(&mut multipart).await?;

    // Download the filter config and run server-side test verification.
    // The Lua sandbox (instruction + memory limits) is the primary defence;
    // the timeout is a last-resort backstop. spawn_blocking tasks cannot be
    // interrupted mid-execution, but the result is discarded on timeout.
    let config = load_filter_config(&state, &hash).await?;
    let cases = validated.cases.clone();
    let handle =
        tokio::task::spawn_blocking(move || crate::verify::verify_filter_server(&config, &cases));
    let verify_result = tokio::time::timeout(std::time::Duration::from_secs(10), handle)
        .await
        .map_err(|_| AppError::BadRequest("test verification timed out (10s limit)".to_string()))?
        .map_err(|e| AppError::Internal(format!("verification task failed: {e}")))?
        .map_err(AppError::BadRequest)?;

    if !verify_result.all_passed() {
        let failures: Vec<String> = verify_result
            .cases
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("test '{}' failed:\n  {}", c.name, c.failures.join("\n  ")))
            .collect();
        return Err(AppError::BadRequest(format!(
            "filter tests failed:\n{}",
            failures.join("\n")
        )));
    }

    // Upload new test files to storage BEFORE touching the DB.
    // If the transaction below fails, these become orphans — harmless and
    // overwritten on retry (storage puts are idempotent by key).
    let test_count = validated.files.len();
    let new_r2_keys = upload_tests(&state, &hash, validated.files).await?;

    // Atomically swap test rows; returns the old R2 keys for cleanup.
    let old_r2_keys = swap_test_rows(&state, &hash, &new_r2_keys).await?;

    // Best-effort cleanup: delete old storage objects AFTER the DB commit
    // succeeds. Failures here leave orphaned blobs but never corrupt state.
    if let Err(e) = storage::delete_tests_for_hash(&*state.storage, &old_r2_keys).await {
        tracing::warn!("failed to delete old test files from storage: {e}");
    }

    let registry_url = format!("{}/filters/{}", state.public_url, hash);
    Ok((
        crate::routes::ip::rate_limit_headers(&rl),
        Json(UpdateTestsResponse {
            content_hash: hash,
            command_pattern,
            author: auth.username,
            test_count,
            registry_url,
        }),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
