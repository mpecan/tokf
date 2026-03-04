use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use tokf_common::config::types::FilterConfig;
use tokf_common::test_case::TestCase;

use crate::auth::service_token::ServiceAuth;
use crate::error::AppError;
use crate::state::AppState;
use crate::storage;

// ── Request / response types ────────────────────────────────────────────────

const fn default_limit() -> usize {
    100
}

const MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct RegenerateRequest {
    /// Specific filter hashes to process. If empty, processes all filters up to `limit`.
    #[serde(default)]
    hashes: Vec<String>,
    /// Max filters to process (default 100, capped at 500).
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Serialize)]
pub struct RegenerateResponse {
    pub processed: usize,
    pub skipped: usize,
    pub failed: Vec<RegenerateFailure>,
}

#[derive(Debug, Serialize)]
pub struct RegenerateFailure {
    pub content_hash: String,
    pub error: String,
}

// ── Handler ─────────────────────────────────────────────────────────────────

/// `POST /api/filters/regenerate-examples` — Regenerate examples and safety
/// checks for existing filters. Requires service token auth.
pub async fn regenerate_examples(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<RegenerateRequest>,
) -> Result<Json<RegenerateResponse>, AppError> {
    let limit = req.limit.min(MAX_LIMIT);

    let mut hashes = if req.hashes.is_empty() {
        fetch_all_hashes(&state.db, limit).await?
    } else {
        req.hashes
    };
    hashes.truncate(MAX_LIMIT);

    let mut processed: usize = 0;
    let mut skipped: usize = 0;
    let mut failed = Vec::new();
    let mut processed_hashes = Vec::new();

    for hash in &hashes {
        match process_filter(&state, hash).await {
            Ok(true) => {
                processed += 1;
                processed_hashes.push(hash.clone());
            }
            Ok(false) => skipped += 1,
            Err(e) => failed.push(RegenerateFailure {
                content_hash: hash.clone(),
                error: e,
            }),
        }
    }

    if !processed_hashes.is_empty() {
        crate::catalog::spawn_batch_catalog_update(
            state.db.clone(),
            state.storage.clone(),
            processed_hashes,
            true,
        );
    }

    Ok(Json(RegenerateResponse {
        processed,
        skipped,
        failed,
    }))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Fetch filters ordered by oldest examples first (never-processed come first).
async fn fetch_all_hashes(pool: &sqlx::PgPool, limit: usize) -> Result<Vec<String>, AppError> {
    #[allow(clippy::cast_possible_wrap)]
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT content_hash FROM filters \
         ORDER BY COALESCE(examples_generated_at, '1970-01-01T00:00:00Z'::TIMESTAMPTZ) ASC \
         LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Process a single filter: fetch from R2, regenerate examples, update DB.
///
/// Returns `Ok(true)` on success, `Ok(false)` if skipped (shouldn't happen),
/// or `Err(message)` on failure.
async fn process_filter(state: &AppState, content_hash: &str) -> Result<bool, String> {
    // 1. Fetch filter's R2 key from DB
    let r2_key: String = sqlx::query_scalar("SELECT r2_key FROM filters WHERE content_hash = $1")
        .bind(content_hash)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| format!("DB error: {e}"))?
        .ok_or_else(|| format!("filter not found: {content_hash}"))?;

    // 2. Fetch filter TOML from R2
    let filter_bytes = state
        .storage
        .get(&r2_key)
        .await
        .map_err(|e| format!("R2 error fetching filter: {e}"))?
        .ok_or_else(|| format!("filter TOML not found in R2: {r2_key}"))?;

    // 3. Parse FilterConfig
    let toml_str = std::str::from_utf8(&filter_bytes)
        .map_err(|_| "filter TOML is not valid UTF-8".to_string())?;
    let config: FilterConfig =
        toml::from_str(toml_str).map_err(|e| format!("invalid filter TOML: {e}"))?;

    // 4. Fetch test file R2 keys from DB
    let test_r2_keys: Vec<String> =
        sqlx::query_scalar("SELECT r2_key FROM filter_tests WHERE filter_hash = $1")
            .bind(content_hash)
            .fetch_all(&state.db)
            .await
            .map_err(|e| format!("DB error fetching tests: {e}"))?;

    // 5. Fetch test bytes from R2 and parse test cases
    let mut test_cases: Vec<TestCase> = Vec::with_capacity(test_r2_keys.len());
    for key in &test_r2_keys {
        let bytes = state
            .storage
            .get(key)
            .await
            .map_err(|e| format!("R2 error fetching test {key}: {e}"))?
            .ok_or_else(|| format!("test file not found in R2: {key}"))?;
        let tc = tokf_common::test_case::validate(&bytes)
            .map_err(|e| format!("invalid test {key}: {e}"))?;
        test_cases.push(tc);
    }

    // 6. Generate examples + safety report
    let (examples_json, safety_passed) =
        super::publish::generate_examples_and_safety(&config, &test_cases).await?;

    // 7. Upload examples to R2
    storage::upload_examples(&*state.storage, content_hash, examples_json)
        .await
        .map_err(|e| format!("R2 error uploading examples: {e}"))?;

    // 8. Update safety_passed and examples_generated_at in DB
    super::publish::update_safety_passed(&state.db, content_hash, safety_passed)
        .await
        .map_err(|e| format!("DB error updating safety: {e}"))?;

    super::publish::set_examples_generated_at(&state.db, content_hash).await;

    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "regenerate_tests.rs"]
mod tests;
