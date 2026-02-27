use axum::{Json, extract::State, http::StatusCode};
use bytes::Bytes;
use serde::Serialize;
use tokf_common::config::types::FilterConfig;
use tokf_common::hash::canonical_hash;
use tokf_common::test_case::TestCase;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::storage;

pub(super) const MAX_FILTER_SIZE: usize = 64 * 1_024;
pub(super) const MAX_TOTAL_SIZE: usize = 1_024 * 1_024;

// ── Response type ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PublishFilterResponse {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub registry_url: String,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

struct MultipartFields {
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
    mit_license_accepted: bool,
}

async fn parse_multipart(
    multipart: &mut axum::extract::Multipart,
) -> Result<MultipartFields, AppError> {
    let mut filter_bytes: Option<Vec<u8>> = None;
    let mut test_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_size: usize = 0;
    let mut mit_license_accepted = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        let Some(name) = field.name() else {
            tracing::debug!("ignoring unnamed multipart field");
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
        if name == "filter" {
            if bytes.len() > MAX_FILTER_SIZE {
                return Err(AppError::BadRequest(
                    "filter TOML exceeds 64 KB size limit".to_string(),
                ));
            }
            filter_bytes = Some(bytes.to_vec());
        } else if name == "mit_license_accepted" {
            mit_license_accepted = bytes.as_ref() == b"true";
        } else if let Some(filename) = name.strip_prefix("test:")
            && !filename.is_empty()
        {
            test_files.push((filename.to_string(), bytes.to_vec()));
        }
    }

    let filter_bytes = filter_bytes
        .ok_or_else(|| AppError::BadRequest("missing required 'filter' field".to_string()))?;
    Ok(MultipartFields {
        filter_bytes,
        test_files,
        mit_license_accepted,
    })
}

/// Grouped fields for inserting a filter record.
struct FilterInsert<'a> {
    content_hash: &'a str,
    command_pattern: &'a str,
    canonical_command: &'a str,
    author_id: i64,
    r2_key: &'a str,
}

/// Returns `(author_username, was_new)` for the filter with `content_hash`.
///
/// Attempts an INSERT and uses the result to distinguish new vs duplicate.
/// Uploading to R2 before this call means orphaned objects are possible on DB
/// failure, but they are harmless (no user-visible state and R2 uploads are
/// idempotent on retry).
async fn upsert_filter_record(
    state: &AppState,
    insert: &FilterInsert<'_>,
    author_username: &str,
) -> Result<(String, bool), AppError> {
    let result = sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (content_hash) DO NOTHING",
    )
    .bind(insert.content_hash)
    .bind(insert.command_pattern)
    .bind(insert.canonical_command)
    .bind(insert.author_id)
    .bind(insert.r2_key)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        // Duplicate — fetch the original author's username
        let existing_author: String = sqlx::query_scalar(
            "SELECT u.username FROM filters f
             JOIN users u ON u.id = f.author_id
             WHERE f.content_hash = $1",
        )
        .bind(insert.content_hash)
        .fetch_one(&state.db)
        .await?;
        return Ok((existing_author, false));
    }

    Ok((author_username.to_string(), true))
}

pub(super) async fn upload_tests(
    state: &AppState,
    content_hash: &str,
    test_files: Vec<(String, Vec<u8>)>,
) -> Result<Vec<String>, AppError> {
    let mut keys = Vec::new();
    for (filename, bytes) in test_files {
        let key = storage::upload_test(&*state.storage, content_hash, &filename, bytes)
            .await
            .map_err(|e| AppError::Internal(format!("storage error for {filename}: {e}")))?;
        keys.push(key);
    }
    Ok(keys)
}

pub(super) async fn insert_filter_tests(
    pool: &sqlx::PgPool,
    content_hash: &str,
    test_r2_keys: &[String],
) -> Result<(), AppError> {
    for key in test_r2_keys {
        sqlx::query("INSERT INTO filter_tests (filter_hash, r2_key) VALUES ($1, $2)")
            .bind(content_hash)
            .bind(key)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Validated, hashed filter ready for storage.
struct PreparedFilter {
    content_hash: String,
    command_pattern: String,
    canonical_command: String,
    config: FilterConfig,
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
    test_cases: Vec<TestCase>,
}

/// Validate the multipart fields, check server-side constraints, and compute
/// the content hash.
///
/// Rejects filters with `lua_script.file` early — before hashing or
/// storage — so invalid uploads never reach R2.
fn prepare_filter(fields: MultipartFields) -> Result<PreparedFilter, AppError> {
    if !fields.mit_license_accepted {
        return Err(AppError::BadRequest(
            "MIT license not accepted — set 'mit_license_accepted' to 'true'".to_string(),
        ));
    }
    let toml_str = std::str::from_utf8(&fields.filter_bytes)
        .map_err(|_| AppError::BadRequest("filter TOML is not valid UTF-8".to_string()))?;
    let config: FilterConfig = toml::from_str(toml_str)
        .map_err(|e| AppError::BadRequest(format!("invalid filter TOML: {e}")))?;

    // Fail fast: reject lua_script.file before computing hash or uploading.
    if let Some(ref script) = config.lua_script
        && script.file.is_some()
    {
        return Err(AppError::BadRequest(
            "lua_script.file is not supported for published filters; \
             use inline 'source' instead (Hint: `tokf publish` inlines \
             file references automatically)"
                .to_string(),
        ));
    }

    let command_pattern = config.command.first().to_string();
    if command_pattern.is_empty() {
        return Err(AppError::BadRequest(
            "filter must have at least one non-empty command".to_string(),
        ));
    }
    let canonical_command = command_pattern
        .split_whitespace()
        .next()
        .unwrap_or(&command_pattern)
        .to_string();

    // Require at least one test file
    if fields.test_files.is_empty() {
        return Err(AppError::BadRequest(
            "at least one test file is required to publish a filter".to_string(),
        ));
    }

    // Validate and parse test files
    let mut test_cases = Vec::with_capacity(fields.test_files.len());
    for (filename, bytes) in &fields.test_files {
        let tc = tokf_common::test_case::validate(bytes)
            .map_err(|e| AppError::BadRequest(format!("{filename}: {e}")))?;
        test_cases.push(tc);
    }

    let content_hash =
        canonical_hash(&config).map_err(|e| AppError::Internal(format!("hash error: {e}")))?;
    Ok(PreparedFilter {
        content_hash,
        command_pattern,
        canonical_command,
        config,
        filter_bytes: fields.filter_bytes,
        test_files: fields.test_files,
        test_cases,
    })
}

// ── POST /api/filters ─────────────────────────────────────────────────────────

/// Run server-side test verification in a blocking task with a timeout.
async fn run_verification(prepared: &PreparedFilter) -> Result<(), AppError> {
    let config = prepared.config.clone();
    let cases = prepared.test_cases.clone();
    let verify_result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::task::spawn_blocking(move || crate::verify::verify_filter_server(&config, &cases)),
    )
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
    Ok(())
}

/// Publish a filter TOML and test files to the community registry.
///
/// Accepts a multipart form with:
/// - `filter` — filter TOML bytes (required, ≤ 64 KB)
/// - `mit_license_accepted` — must be `"true"` to acknowledge MIT license (required)
/// - `test:<filename>` — individual test TOML files (required, ≥ 1, total upload ≤ 1 MB)
///
/// The server computes the content hash from the uploaded bytes; clients never
/// supply a hash. This prevents hash forgery.
///
/// Returns `201 Created` on first publish, `200 OK` if the same hash was
/// already in the registry (idempotent).
///
/// # Errors
///
/// - `400 Bad Request` if the multipart is malformed, the TOML is invalid,
///   size limits are exceeded, or MIT license was not accepted.
/// - `401 Unauthorized` if the bearer token is missing or invalid.
/// - `429 Too Many Requests` if the user exceeds publish rate limits.
/// - `500 Internal Server Error` on storage or database failures.
pub async fn publish_filter(
    auth: AuthUser,
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<(StatusCode, Json<PublishFilterResponse>), AppError> {
    if !state.publish_rate_limiter.check_and_increment(auth.user_id) {
        return Err(AppError::RateLimited);
    }

    let fields = parse_multipart(&mut multipart).await?;
    let prepared = prepare_filter(fields)?;

    // Run server-side test verification before persisting anything.
    run_verification(&prepared).await?;

    // Upload to R2 first (idempotent). If the DB insert below fails, the R2
    // object is orphaned but harmless — no user-visible state changes, and
    // the upload is retried correctly on the next publish attempt.
    let r2_key = storage::upload_filter(
        &*state.storage,
        &prepared.content_hash,
        prepared.filter_bytes,
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let test_r2_keys = upload_tests(&state, &prepared.content_hash, prepared.test_files).await?;

    let insert = FilterInsert {
        content_hash: &prepared.content_hash,
        command_pattern: &prepared.command_pattern,
        canonical_command: &prepared.canonical_command,
        author_id: auth.user_id,
        r2_key: &r2_key,
    };
    let (author, is_new) = upsert_filter_record(&state, &insert, &auth.username).await?;

    if is_new {
        insert_filter_tests(&state.db, &prepared.content_hash, &test_r2_keys).await?;
    }

    let registry_url = format!("{}/filters/{}", state.public_url, prepared.content_hash);
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(PublishFilterResponse {
            content_hash: prepared.content_hash,
            command_pattern: prepared.command_pattern,
            author,
            registry_url,
        }),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
