use axum::{Json, extract::State};
use serde::Serialize;

use crate::auth::service_token::ServiceAuth;
use crate::catalog;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub filters_count: usize,
    pub generated_at: String,
}

/// `POST /api/catalog/refresh` — Rebuild the full catalog and write to R2.
///
/// Requires service token auth. Idempotent — safe to call multiple times.
/// Used for manual reconciliation after data fixes.
pub async fn refresh_catalog(
    _auth: ServiceAuth,
    State(state): State<AppState>,
) -> Result<Json<RefreshResponse>, AppError> {
    let index = catalog::refresh_catalog(&state.db, &*state.storage).await?;

    Ok(Json(RefreshResponse {
        filters_count: index.filters.len(),
        generated_at: index.generated_at,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "catalog_tests.rs"]
mod tests;
