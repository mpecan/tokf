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
pub(super) const MAX_TEST_FILES: usize = 50;

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
    safety_passed: bool,
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
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key, safety_passed)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (content_hash) DO NOTHING",
    )
    .bind(insert.content_hash)
    .bind(insert.command_pattern)
    .bind(insert.canonical_command)
    .bind(insert.author_id)
    .bind(insert.r2_key)
    .bind(insert.safety_passed)
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

pub async fn upload_tests(
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

pub async fn insert_filter_tests(
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

/// Update the `safety_passed` column for a published filter.
pub(super) async fn update_safety_passed(
    pool: &sqlx::PgPool,
    content_hash: &str,
    passed: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE filters SET safety_passed = $1 WHERE content_hash = $2")
        .bind(passed)
        .bind(content_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Mark a filter's examples as generated (best-effort, logs on failure).
pub(super) async fn set_examples_generated_at(pool: &sqlx::PgPool, content_hash: &str) {
    if let Err(e) =
        sqlx::query("UPDATE filters SET examples_generated_at = NOW() WHERE content_hash = $1")
            .bind(content_hash)
            .execute(pool)
            .await
    {
        tracing::warn!(hash = %content_hash, "failed to set examples_generated_at: {e}");
    }
}

/// Validated, hashed filter ready for storage.
pub(super) struct PreparedFilter {
    pub(super) content_hash: String,
    pub(super) command_pattern: String,
    pub(super) canonical_command: String,
    pub(super) config: FilterConfig,
    pub(super) filter_bytes: Vec<u8>,
    pub(super) test_files: Vec<(String, Vec<u8>)>,
    pub(super) test_cases: Vec<TestCase>,
}

/// Validate filter TOML and test files, compute the canonical content hash.
///
/// Shared by both regular publish and stdlib publish. Rejects filters with
/// `lua_script.file`, empty commands, or missing tests.
pub(super) fn validate_and_prepare(
    filter_toml: &[u8],
    test_files: Vec<(String, Vec<u8>)>,
) -> Result<PreparedFilter, String> {
    let toml_str = std::str::from_utf8(filter_toml)
        .map_err(|_| "filter TOML is not valid UTF-8".to_string())?;
    let config: FilterConfig =
        toml::from_str(toml_str).map_err(|e| format!("invalid filter TOML: {e}"))?;

    if let Some(ref script) = config.lua_script
        && script.file.is_some()
    {
        return Err("lua_script.file is not supported for published filters; \
             use inline 'source' instead (Hint: `tokf publish` inlines \
             file references automatically)"
            .to_string());
    }

    let command_pattern = config.command.first().to_string();
    if command_pattern.is_empty() {
        return Err("filter must have at least one non-empty command".to_string());
    }
    let canonical_command = command_pattern
        .split_whitespace()
        .next()
        .unwrap_or(&command_pattern)
        .to_string();

    if test_files.is_empty() {
        return Err("at least one test file is required to publish a filter".to_string());
    }
    if test_files.len() > MAX_TEST_FILES {
        return Err(format!(
            "too many test files ({}, max {MAX_TEST_FILES})",
            test_files.len()
        ));
    }

    let mut test_cases = Vec::with_capacity(test_files.len());
    for (filename, bytes) in &test_files {
        let tc = tokf_common::test_case::validate(bytes).map_err(|e| format!("{filename}: {e}"))?;
        test_cases.push(tc);
    }

    let content_hash = canonical_hash(&config).map_err(|e| format!("hash error: {e}"))?;
    Ok(PreparedFilter {
        content_hash,
        command_pattern,
        canonical_command,
        config,
        filter_bytes: filter_toml.to_vec(),
        test_files,
        test_cases,
    })
}

/// Validate the multipart fields, check server-side constraints, and compute
/// the content hash. Delegates to `validate_and_prepare` after MIT license check.
fn prepare_filter(fields: MultipartFields) -> Result<PreparedFilter, AppError> {
    if !fields.mit_license_accepted {
        return Err(AppError::BadRequest(
            "MIT license not accepted — set 'mit_license_accepted' to 'true'".to_string(),
        ));
    }
    validate_and_prepare(&fields.filter_bytes, fields.test_files).map_err(AppError::BadRequest)
}

// ── POST /api/filters ─────────────────────────────────────────────────────────

/// Run server-side test verification in a blocking task with a timeout.
///
/// On timeout, the `JoinHandle` is dropped so the caller does not wait
/// indefinitely. Note: `spawn_blocking` tasks cannot be preempted — the Lua
/// sandbox (instruction + memory limits) is the primary defence against
/// runaway scripts; this timeout is a last-resort backstop for the caller.
pub async fn run_verification(config: &FilterConfig, cases: &[TestCase]) -> Result<(), String> {
    let config = config.clone();
    let cases = cases.to_vec();
    let handle =
        tokio::task::spawn_blocking(move || crate::verify::verify_filter_server(&config, &cases));
    let timeout = std::time::Duration::from_secs(10);
    let verify_result = tokio::select! {
        result = handle => {
            result.map_err(|e| format!("verification task failed: {e}"))??
        }
        () = tokio::time::sleep(timeout) => {
            return Err("test verification timed out (10s limit)".to_string());
        }
    };

    if !verify_result.all_passed() {
        let failures: Vec<String> = verify_result
            .cases
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("test '{}' failed:\n  {}", c.name, c.failures.join("\n  ")))
            .collect();
        return Err(format!("filter tests failed:\n{}", failures.join("\n")));
    }
    Ok(())
}

/// Generate before/after examples and safety report in a blocking task.
///
/// Returns `(examples_json, safety_passed)`. Runs with sandboxed Lua limits.
/// On timeout, the `JoinHandle` is dropped so the caller does not wait
/// indefinitely. Note: `spawn_blocking` tasks cannot be preempted — the Lua
/// sandbox (instruction + memory limits) is the primary defence against
/// runaway closures; this timeout is a last-resort backstop for the caller.
pub async fn generate_examples_and_safety(
    config: &FilterConfig,
    cases: &[TestCase],
) -> Result<(Vec<u8>, bool), String> {
    let config = config.clone();
    let cases = cases.to_vec();
    let handle = tokio::task::spawn_blocking(move || {
        let examples = tokf_filter::examples::generate_examples_sandboxed(
            &config,
            &cases,
            &crate::verify::server_lua_limits(),
        );
        let safety_passed = examples.safety.passed;
        let json = serde_json::to_vec(&examples).map_err(|e| format!("JSON error: {e}"))?;
        Ok::<_, String>((json, safety_passed))
    });
    let timeout = std::time::Duration::from_secs(10);
    tokio::select! {
        result = handle => {
            result.map_err(|e| format!("example generation task failed: {e}"))?
        }
        () = tokio::time::sleep(timeout) => {
            Err("example generation timed out (10s limit)".to_string())
        }
    }
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
#[allow(clippy::too_many_lines)]
pub async fn publish_filter(
    auth: AuthUser,
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<
    (
        StatusCode,
        axum::http::HeaderMap,
        Json<PublishFilterResponse>,
    ),
    AppError,
> {
    let rl = state.publish_rate_limiter.check_and_increment(auth.user_id);
    if !rl.allowed {
        return Err(AppError::rate_limited(&rl));
    }

    let fields = parse_multipart(&mut multipart).await?;
    let prepared = prepare_filter(fields)?;

    // Run server-side test verification before persisting anything.
    run_verification(&prepared.config, &prepared.test_cases)
        .await
        .map_err(AppError::BadRequest)?;

    // Generate before/after examples + safety report (best-effort; failures don't block publish).
    let examples_result =
        generate_examples_and_safety(&prepared.config, &prepared.test_cases).await;
    if let Err(ref e) = examples_result {
        tracing::warn!(hash = %prepared.content_hash, "example generation failed (continuing): {e}");
    }
    let safety_passed = examples_result.as_ref().map_or(true, |(_, sp)| *sp);

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
        safety_passed,
    };
    let (author, is_new) = upsert_filter_record(&state, &insert, &auth.username).await?;

    // Upload examples to R2 AFTER insert so set_examples_generated_at finds the row.
    if let Ok((examples_json, _)) = examples_result {
        if let Err(e) =
            storage::upload_examples(&*state.storage, &prepared.content_hash, examples_json).await
        {
            tracing::warn!(hash = %prepared.content_hash, "failed to upload examples: {e}");
        } else {
            set_examples_generated_at(&state.db, &prepared.content_hash).await;
        }
    }

    if is_new {
        insert_filter_tests(&state.db, &prepared.content_hash, &test_r2_keys).await?;

        // Fire-and-forget: materialize per-filter metadata + catalog index to R2
        crate::catalog::spawn_catalog_update(
            state.db.clone(),
            state.storage.clone(),
            prepared.content_hash.clone(),
        );
    }

    let registry_url = format!("{}/filters/{}", state.public_url, prepared.content_hash);
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        crate::routes::ip::rate_limit_headers(&rl),
        Json(PublishFilterResponse {
            content_hash: prepared.content_hash,
            command_pattern: prepared.command_pattern,
            author,
            registry_url,
        }),
    ))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

pub mod stdlib;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
