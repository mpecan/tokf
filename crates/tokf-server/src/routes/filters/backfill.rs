use axum::{Json, extract::State, http::StatusCode};
use futures_util::{StreamExt, stream};
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

/// How many rows are fetched-and-hashed at once. The work is I/O-bound on R2,
/// so serial processing made a 300-row batch take minutes and get cancelled by
/// client timeouts before the handler could reply.
const V1_CONCURRENCY: usize = 16;

/// Emit a progress log every N completed rows, so a stalled batch is visible
/// rather than a silent gap between "request received" and "complete".
const V1_PROGRESS_INTERVAL: usize = 25;

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
/// `v1_hash IS NULL`, ordered by `updated_at` (oldest attempt first),
/// fetches the TOML from R2, computes the v1 hash (ADR-0002), writes it
/// back. Rows whose R2 object is missing or whose TOML fails to
/// canonicalise are reported in `failed` and skipped — but their
/// `updated_at` is still bumped so they rotate to the back of the queue
/// and don't starve newer rows.
///
/// Rows are processed [`V1_CONCURRENCY`] at a time — the work is I/O-bound on
/// R2 and a serial batch is slow enough that clients time out and cancel the
/// handler before it can reply.
///
/// Operator runbook: invoke repeatedly until `updated == 0`. A response
/// with `updated == 0` and a non-empty `failed` list means only
/// permanently-failing rows remain — triage those manually; further calls
/// will not make progress. Failures don't stop the batch.
///
/// Callers must allow a generous request timeout (the `tokf` CLI defaults to
/// 5s; the backfill workflow raises it via `TOKF_HTTP_TIMEOUT`). A cancelled
/// request loses no work — each row commits its own `UPDATE` — but the
/// response, and the `failed` list with it, is lost.
pub async fn backfill_v1_hashes(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<BackfillV1Request>,
) -> Result<(StatusCode, Json<BackfillV1Response>), AppError> {
    let limit = req.limit.clamp(1, MAX_BACKFILL_BATCH);
    // Safe: `limit` is clamped to [1, MAX_BACKFILL_BATCH] (= 500) which fits in i64.
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    // Order by `updated_at` (not `created_at`): every attempt bumps it, so a
    // permanently-failing row rotates to the back instead of blocking the head
    // of the scan. `content_hash` is a deterministic tie-break for rows that
    // share an `updated_at` (e.g. all legacy rows after the migration aligned
    // them to `created_at`).
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT content_hash, r2_key FROM filters
         WHERE v1_hash IS NULL
         ORDER BY updated_at ASC, content_hash ASC
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

    let (updated, failed) = process_v1_batch(&state, &rows).await;

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

/// Fetch, hash and store `v1_hash` for every row in `rows`, up to
/// [`V1_CONCURRENCY`] at a time. Returns `(updated, failures)`.
///
/// Failures never abort the batch: the row's `updated_at` is bumped so it
/// rotates to the back of the cursor, and it is reported to the caller.
/// Progress is logged every [`V1_PROGRESS_INTERVAL`] completions so a batch
/// that stalls mid-flight is diagnosable from the logs alone.
async fn process_v1_batch(
    state: &AppState,
    rows: &[(String, String)],
) -> (usize, Vec<BackfillV1Failure>) {
    let total = rows.len();
    // Each future owns its inputs (cloned `AppState`, owned hash/key) rather
    // than borrowing from the enclosing frame: a borrowing async block here
    // makes the handler future fail axum's higher-ranked lifetime check.
    let mut results = stream::iter(rows.to_vec())
        .map(|(content_hash, r2_key)| {
            let state = state.clone();
            async move {
                match compute_and_store_v1(&state, &content_hash, &r2_key).await {
                    Ok(()) => None,
                    Err(e) => {
                        tracing::warn!(hash = %content_hash, "backfill-v1 failed: {e}");
                        // Bump `updated_at` so this row rotates to the back of
                        // the cursor and doesn't starve newer rows on the next
                        // call. The row's `v1_hash` stays NULL, so it remains
                        // a candidate.
                        touch_updated_at(&state, &content_hash).await;
                        Some(BackfillV1Failure {
                            content_hash,
                            error: e.to_string(),
                        })
                    }
                }
            }
        })
        .buffer_unordered(V1_CONCURRENCY);

    let mut updated = 0usize;
    let mut failed = Vec::new();
    let mut done = 0usize;
    while let Some(outcome) = results.next().await {
        match outcome {
            None => updated += 1,
            Some(f) => failed.push(f),
        }
        done += 1;
        if done.is_multiple_of(V1_PROGRESS_INTERVAL) && done < total {
            tracing::info!(
                done,
                total,
                updated,
                failed = failed.len(),
                "backfill-v1-hashes progress"
            );
        }
    }

    (updated, failed)
}

async fn compute_and_store_v1(
    state: &AppState,
    content_hash: &str,
    r2_key: &str,
) -> anyhow::Result<()> {
    let toml_str = crate::storage::get_utf8(&*state.storage, r2_key).await?;
    let v1 = tokf_common::canonical_v1::hash(&toml_str)?;
    sqlx::query("UPDATE filters SET v1_hash = $1, updated_at = NOW() WHERE content_hash = $2")
        .bind(&v1)
        .bind(content_hash)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Bump a row's `updated_at` to `NOW()` without touching `v1_hash`. Used on
/// backfill failure so the row rotates to the back of the `updated_at` cursor.
/// Best-effort: a failure here only means the row keeps its old `updated_at`
/// and may be retried sooner, so we log rather than propagate.
async fn touch_updated_at(state: &AppState, content_hash: &str) {
    if let Err(e) = sqlx::query("UPDATE filters SET updated_at = NOW() WHERE content_hash = $1")
        .bind(content_hash)
        .execute(&state.db)
        .await
    {
        tracing::warn!(hash = %content_hash, "failed to bump updated_at after backfill failure: {e}");
    }
}
