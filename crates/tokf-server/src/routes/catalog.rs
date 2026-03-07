use axum::{Json, extract::State};
use serde::Serialize;

use crate::auth::service_token::ServiceAuth;
use crate::auth::token::AuthUser;
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

/// `GET /api/catalog/grouped` — Return the command-grouped catalog.
///
/// Requires bearer token auth. Serves from R2 cache when available,
/// falling back to a fresh DB build.
pub async fn get_grouped_catalog(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<catalog::GroupedCatalog>, AppError> {
    // Try R2 cache first.
    if let Ok(Some(bytes)) = state.storage.get(catalog::grouped_catalog_key()).await
        && let Ok(cached) = serde_json::from_slice::<catalog::GroupedCatalog>(&bytes)
    {
        return Ok(Json(cached));
    }

    // Fallback: build fresh from DB.
    let grouped = catalog::build_grouped_catalog(&state.db).await?;
    Ok(Json(grouped))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "catalog_tests.rs"]
mod tests;
