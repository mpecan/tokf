use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::auth::service_token::ServiceAuth;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct BackfillVersionsRequest {
    pub entries: Vec<BackfillEntry>,
}

#[derive(Debug, Deserialize)]
pub struct BackfillEntry {
    pub content_hash: String,
    #[serde(default)]
    pub introduced_at: Option<String>,
    #[serde(default)]
    pub deprecated_at: Option<String>,
    #[serde(default)]
    pub successor_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BackfillVersionsResponse {
    pub updated: usize,
    pub skipped: usize,
}

/// Maximum batch size for either backfill endpoint. Bounds operator load
/// against the DB / R2.
const MAX_BACKFILL_BATCH: usize = 500;

/// `POST /api/filters/backfill-versions` — Backfill version data for filters.
///
/// Service-token auth only. Idempotent — repeated runs overwrite with same values.
/// Skips hashes not present in the DB.
pub async fn backfill_versions(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<BackfillVersionsRequest>,
) -> Result<(StatusCode, Json<BackfillVersionsResponse>), AppError> {
    if req.entries.len() > MAX_BACKFILL_BATCH {
        return Err(AppError::BadRequest(format!(
            "batch size {} exceeds maximum of {MAX_BACKFILL_BATCH}",
            req.entries.len()
        )));
    }

    tracing::info!(
        count = req.entries.len(),
        "backfill-versions request received"
    );

    let mut updated = 0usize;
    let mut skipped = 0usize;

    for entry in &req.entries {
        let result = sqlx::query(
            "UPDATE filters
             SET introduced_at = $2, deprecated_at = $3, successor_hash = $4
             WHERE content_hash = $1",
        )
        .bind(&entry.content_hash)
        .bind(&entry.introduced_at)
        .bind(&entry.deprecated_at)
        .bind(&entry.successor_hash)
        .execute(&state.db)
        .await?;

        if result.rows_affected() > 0 {
            updated += 1;
        } else {
            skipped += 1;
        }
    }

    tracing::info!(updated, skipped, "backfill-versions complete");

    // Trigger grouped catalog refresh after backfill.
    if updated > 0 {
        let pool = state.db.clone();
        let storage = state.storage.clone();
        tokio::spawn(async move {
            match crate::catalog::build_grouped_catalog(&pool).await {
                Ok(grouped) => {
                    if let Err(e) =
                        crate::catalog::write_grouped_catalog_to_r2(&*storage, &grouped).await
                    {
                        tracing::warn!("backfill grouped catalog write failed: {e}");
                    }
                }
                Err(e) => tracing::warn!("backfill grouped catalog build failed: {e}"),
            }
        });
    }

    Ok((
        StatusCode::OK,
        Json(BackfillVersionsResponse { updated, skipped }),
    ))
}

// ── v1_hash backfill ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BackfillV1Request {
    /// Maximum rows to process in this call. Operator iterates until
    /// `processed == 0`. Defaults to 100; capped at `MAX_BACKFILL_BATCH`.
    #[serde(default = "default_v1_batch_size")]
    pub limit: usize,
}

const fn default_v1_batch_size() -> usize {
    100
}

#[derive(Debug, Serialize)]
pub struct BackfillV1Response {
    pub processed: usize,
    pub updated: usize,
    pub failed: Vec<BackfillV1Failure>,
}

#[derive(Debug, Serialize)]
pub struct BackfillV1Failure {
    pub content_hash: String,
    pub error: String,
}

/// `POST /api/filters/backfill-v1-hashes` — populate `v1_hash` for legacy rows.
///
/// Service-token auth. Idempotent. Picks up to `limit` rows where
/// `v1_hash IS NULL`, fetches the TOML from R2, computes the v1 hash
/// (ADR-0002), writes it back. Rows whose R2 object is missing or whose
/// TOML fails to canonicalise are reported in `failed` and skipped — the
/// operator inspects the response and triages those manually.
///
/// Operator runbook: invoke repeatedly until `processed == 0`. Failures
/// don't stop the batch.
pub async fn backfill_v1_hashes(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<BackfillV1Request>,
) -> Result<(StatusCode, Json<BackfillV1Response>), AppError> {
    let limit = req.limit.clamp(1, MAX_BACKFILL_BATCH);
    // Safe: `limit` is clamped to [1, MAX_BACKFILL_BATCH] (= 500) which fits in i64.
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT content_hash, r2_key FROM filters
         WHERE v1_hash IS NULL
         ORDER BY created_at ASC
         LIMIT $1",
    )
    .bind(limit_i64)
    .fetch_all(&state.db)
    .await?;

    tracing::info!(
        candidates = rows.len(),
        limit,
        "backfill-v1-hashes request received"
    );

    let mut updated = 0usize;
    let mut failed = Vec::new();

    for (content_hash, r2_key) in &rows {
        match compute_and_store_v1(&state, content_hash, r2_key).await {
            Ok(()) => updated += 1,
            Err(e) => {
                tracing::warn!(hash = %content_hash, "backfill-v1 failed: {e}");
                failed.push(BackfillV1Failure {
                    content_hash: content_hash.clone(),
                    error: e.to_string(),
                });
            }
        }
    }

    tracing::info!(
        processed = rows.len(),
        updated,
        failed = failed.len(),
        "backfill-v1-hashes complete"
    );

    Ok((
        StatusCode::OK,
        Json(BackfillV1Response {
            processed: rows.len(),
            updated,
            failed,
        }),
    ))
}

async fn compute_and_store_v1(
    state: &AppState,
    content_hash: &str,
    r2_key: &str,
) -> anyhow::Result<()> {
    let toml_str = crate::storage::get_utf8(&*state.storage, r2_key).await?;
    let v1 = tokf_common::canonical_v1::hash(&toml_str)?;
    sqlx::query("UPDATE filters SET v1_hash = $1 WHERE content_hash = $2")
        .bind(&v1)
        .bind(content_hash)
        .execute(&state.db)
        .await?;
    Ok(())
}
