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

/// Maximum number of entries in a single backfill request.
const MAX_BACKFILL_ENTRIES: usize = 500;

/// `POST /api/filters/backfill-versions` — Backfill version data for filters.
///
/// Service-token auth only. Idempotent — repeated runs overwrite with same values.
/// Skips hashes not present in the DB.
pub async fn backfill_versions(
    _auth: ServiceAuth,
    State(state): State<AppState>,
    Json(req): Json<BackfillVersionsRequest>,
) -> Result<(StatusCode, Json<BackfillVersionsResponse>), AppError> {
    if req.entries.len() > MAX_BACKFILL_ENTRIES {
        return Err(AppError::BadRequest(format!(
            "batch size {} exceeds maximum of {MAX_BACKFILL_ENTRIES}",
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
