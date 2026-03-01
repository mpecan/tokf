use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use tokf_common::config::types::FilterConfig;
use tokf_common::hash::canonical_hash;
use tokf_common::test_case::TestCase;

use crate::auth::service_token::ServiceAuth;
use crate::error::AppError;
use crate::state::AppState;
use crate::storage;

use super::{insert_filter_tests, run_verification, upload_tests};

// ── Request types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct StdlibPublishRequest {
    pub filters: Vec<StdlibFilterEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StdlibFilterEntry {
    pub filter_toml: String,
    pub test_files: Vec<StdlibTestFile>,
    pub author_github_username: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StdlibTestFile {
    pub filename: String,
    pub content: String,
}

// ── Response types ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StdlibPublishResponse {
    pub published: usize,
    pub skipped: usize,
    pub failed: Vec<StdlibFailure>,
}

#[derive(Debug, Serialize)]
pub struct StdlibFailure {
    pub command_pattern: String,
    pub error: String,
}

// ── Constants ────────────────────────────────────────────────────────────────

/// Maximum number of filters in a single publish-stdlib request.
const MAX_BATCH_SIZE: usize = 200;

// ── Internal types ───────────────────────────────────────────────────────────

struct PreparedStdlibFilter {
    content_hash: String,
    command_pattern: String,
    canonical_command: String,
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
    config: FilterConfig,
    test_cases: Vec<TestCase>,
}

/// GitHub usernames: alphanumeric + hyphens, 1-39 chars, no leading/trailing hyphens.
fn validate_github_username(username: &str) -> Result<(), String> {
    if username.is_empty() || username.len() > 39 {
        return Err(format!(
            "invalid GitHub username '{username}': must be 1-39 characters"
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!(
            "invalid GitHub username '{username}': only alphanumeric and hyphens allowed"
        ));
    }
    if username.starts_with('-') || username.ends_with('-') {
        return Err(format!(
            "invalid GitHub username '{username}': cannot start or end with hyphen"
        ));
    }
    Ok(())
}

fn prepare_stdlib_filter(entry: &StdlibFilterEntry) -> Result<PreparedStdlibFilter, String> {
    validate_github_username(&entry.author_github_username)?;

    let config: FilterConfig =
        toml::from_str(&entry.filter_toml).map_err(|e| format!("invalid filter TOML: {e}"))?;

    if let Some(ref script) = config.lua_script
        && script.file.is_some()
    {
        return Err(
            "lua_script.file is not supported for published filters; use inline 'source' instead"
                .to_string(),
        );
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

    if entry.test_files.is_empty() {
        return Err("at least one test file is required".to_string());
    }

    let mut test_cases = Vec::with_capacity(entry.test_files.len());
    let mut test_file_bytes = Vec::with_capacity(entry.test_files.len());
    for tf in &entry.test_files {
        let tc = tokf_common::test_case::validate(tf.content.as_bytes())
            .map_err(|e| format!("{}: {e}", tf.filename))?;
        test_cases.push(tc);
        test_file_bytes.push((tf.filename.clone(), tf.content.as_bytes().to_vec()));
    }

    let content_hash = canonical_hash(&config).map_err(|e| format!("hash error: {e}"))?;

    Ok(PreparedStdlibFilter {
        content_hash,
        command_pattern,
        canonical_command,
        filter_bytes: entry.filter_toml.as_bytes().to_vec(),
        test_files: test_file_bytes,
        config,
        test_cases,
    })
}

/// Find or create a user by GitHub username for stdlib attribution.
///
/// If a user with the given username exists, returns their ID.
/// Otherwise, creates a "ghost" account with `visible = FALSE` and a
/// synthetic negative `github_id` derived from the username hash to avoid
/// conflicts with real GitHub IDs.
async fn upsert_user_by_username(
    db: &sqlx::PgPool,
    github_username: &str,
) -> Result<i64, AppError> {
    // Try to find existing user first
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(github_username)
        .fetch_optional(db)
        .await?;

    if let Some(id) = existing {
        return Ok(id);
    }

    // Create a ghost account with a synthetic negative github_id
    let synthetic_github_id = synthetic_github_id(github_username);
    let avatar_url = format!("https://github.com/{github_username}.png");
    let profile_url = format!("https://github.com/{github_username}");

    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url, visible)
         VALUES ($1, $2, $3, $4, FALSE)
         ON CONFLICT (github_id) DO UPDATE SET username = EXCLUDED.username
         RETURNING id",
    )
    .bind(synthetic_github_id)
    .bind(github_username)
    .bind(&avatar_url)
    .bind(&profile_url)
    .fetch_one(db)
    .await?;

    Ok(user_id)
}

/// Derive a deterministic negative i64 from a username to use as a
/// synthetic `github_id` for ghost accounts. Negative values can't collide
/// with real GitHub IDs (which are positive).
#[allow(clippy::cast_possible_wrap)]
fn synthetic_github_id(username: &str) -> i64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(username.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let val = i64::from_be_bytes(bytes).unsigned_abs();
    -((val % (i64::MAX as u64)) as i64) - 1
}

// ── POST /api/filters/publish-stdlib ─────────────────────────────────────────

/// Publish stdlib filters to the registry in batch.
///
/// Accepts a JSON body with an array of filters. Each filter is validated,
/// verified, and published idempotently. Requires service token auth.
///
/// Returns a summary of published/skipped/failed counts.
pub async fn publish_stdlib(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<StdlibPublishRequest>,
) -> Result<(StatusCode, Json<StdlibPublishResponse>), AppError> {
    tracing::info!(count = req.filters.len(), "publish-stdlib request received");

    if req.filters.len() > MAX_BATCH_SIZE {
        return Err(AppError::BadRequest(format!(
            "batch size {} exceeds maximum of {MAX_BATCH_SIZE}",
            req.filters.len()
        )));
    }

    let mut published = 0usize;
    let mut skipped = 0usize;
    let mut failed = Vec::new();

    for entry in &req.filters {
        match process_entry(&state, entry, &mut skipped).await {
            Ok(true) => published += 1,
            Ok(false) => {} // skipped — already counted inside process_entry
            Err(e) => {
                tracing::warn!(
                    command = %e.command_pattern,
                    error = %e.error,
                    "stdlib filter failed"
                );
                failed.push(e);
            }
        }
    }

    tracing::info!(
        published,
        skipped,
        failed = failed.len(),
        "publish-stdlib complete"
    );

    let status = if failed.is_empty() {
        if published > 0 {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    } else {
        StatusCode::MULTI_STATUS
    };

    Ok((
        status,
        Json(StdlibPublishResponse {
            published,
            skipped,
            failed,
        }),
    ))
}

/// Process a single stdlib filter entry.
///
/// Returns `Ok(true)` when published, `Ok(false)` when skipped (increments
/// `*skipped`), or `Err(StdlibFailure)` when validation/verification fails.
async fn process_entry(
    state: &AppState,
    entry: &StdlibFilterEntry,
    skipped: &mut usize,
) -> Result<bool, StdlibFailure> {
    let cmd_hint = guess_command_pattern(&entry.filter_toml);
    let prepared =
        prepare_stdlib_filter(entry).map_err(|e| fail(&cmd_hint, "preparation failed", e))?;

    if check_existing(state, &prepared, skipped).await? {
        return Ok(false);
    }

    run_verification(&prepared.config, &prepared.test_cases)
        .await
        .map_err(|e| fail(&prepared.command_pattern, "verification failed", e))?;

    persist_filter(state, &prepared, &entry.author_github_username)
        .await
        .map_err(|e| fail(&prepared.command_pattern, "persist failed", e.to_string()))?;

    Ok(true)
}

/// Build a `StdlibFailure` and log a warning in one call.
fn fail(command_pattern: &str, phase: &str, error: String) -> StdlibFailure {
    tracing::warn!(command = %command_pattern, error = %error, "stdlib {phase}");
    StdlibFailure {
        command_pattern: command_pattern.to_string(),
        error,
    }
}

/// Returns `Ok(true)` if the filter already exists (and updates `is_stdlib`).
async fn check_existing(
    state: &AppState,
    prepared: &PreparedStdlibFilter,
    skipped: &mut usize,
) -> Result<bool, StdlibFailure> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT content_hash FROM filters WHERE content_hash = $1")
            .bind(&prepared.content_hash)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| fail(&prepared.command_pattern, "db lookup", e.to_string()))?;

    if existing.is_some() {
        if let Err(e) = sqlx::query("UPDATE filters SET is_stdlib = TRUE WHERE content_hash = $1")
            .bind(&prepared.content_hash)
            .execute(&state.db)
            .await
        {
            tracing::warn!(hash = %prepared.content_hash, "failed to set is_stdlib: {e}");
        }
        *skipped += 1;
        return Ok(true);
    }

    Ok(false)
}

/// Upload to storage and insert DB records for a validated stdlib filter.
async fn persist_filter(
    state: &AppState,
    prepared: &PreparedStdlibFilter,
    author_username: &str,
) -> Result<(), AppError> {
    let author_id = upsert_user_by_username(&state.db, author_username).await?;

    let r2_key = storage::upload_filter(
        &*state.storage,
        &prepared.content_hash,
        prepared.filter_bytes.clone(),
    )
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let test_r2_keys =
        upload_tests(state, &prepared.content_hash, prepared.test_files.clone()).await?;

    sqlx::query(
        "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key, is_stdlib)
         VALUES ($1, $2, $3, $4, $5, TRUE)
         ON CONFLICT (content_hash) DO UPDATE SET is_stdlib = TRUE",
    )
    .bind(&prepared.content_hash)
    .bind(&prepared.command_pattern)
    .bind(&prepared.canonical_command)
    .bind(author_id)
    .bind(&r2_key)
    .execute(&state.db)
    .await?;

    insert_filter_tests(&state.db, &prepared.content_hash, &test_r2_keys).await?;
    Ok(())
}

/// Best-effort command pattern extraction for error messages.
fn guess_command_pattern(toml_str: &str) -> String {
    toml::from_str::<FilterConfig>(toml_str).map_or_else(
        |_| "<unknown>".to_string(),
        |c| c.command.first().to_string(),
    )
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn synthetic_github_id_is_always_negative() {
        for name in ["alice", "bob", "mpecan", "tokf-bot", "x"] {
            let id = synthetic_github_id(name);
            assert!(
                id < 0,
                "synthetic id for {name} should be negative, got {id}"
            );
        }
    }

    #[test]
    fn synthetic_github_id_is_deterministic() {
        assert_eq!(synthetic_github_id("alice"), synthetic_github_id("alice"));
    }

    #[test]
    fn synthetic_github_id_varies_by_username() {
        assert_ne!(synthetic_github_id("alice"), synthetic_github_id("bob"));
    }

    #[test]
    fn guess_command_pattern_parses_valid_toml() {
        let p = guess_command_pattern("command = \"git push\"\n");
        assert_eq!(p, "git push");
    }

    #[test]
    fn guess_command_pattern_returns_unknown_for_invalid() {
        let p = guess_command_pattern("not valid [[[");
        assert_eq!(p, "<unknown>");
    }

    #[test]
    fn validate_github_username_accepts_valid() {
        assert!(validate_github_username("alice").is_ok());
        assert!(validate_github_username("a-b-c").is_ok());
        assert!(validate_github_username("x").is_ok());
    }

    #[test]
    fn validate_github_username_rejects_empty() {
        assert!(validate_github_username("").is_err());
    }

    #[test]
    fn validate_github_username_rejects_leading_hyphen() {
        assert!(validate_github_username("-alice").is_err());
    }

    #[test]
    fn validate_github_username_rejects_trailing_hyphen() {
        assert!(validate_github_username("alice-").is_err());
    }

    #[test]
    fn validate_github_username_rejects_special_chars() {
        assert!(validate_github_username("alice@bob").is_err());
        assert!(validate_github_username("alice bob").is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
