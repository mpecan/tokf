use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use super::token::hash_token;
use crate::error::AppError;

/// Authenticated service token extracted from `Authorization: Bearer <token>`.
///
/// Unlike [`super::token::AuthUser`], this is not tied to a user account.
/// It validates against the `service_tokens` table for CI automation.
#[derive(Debug, Clone)]
pub struct ServiceAuth;

impl FromRequestParts<crate::state::AppState> for ServiceAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized)?;
        let token_hash = hash_token(token);

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM service_tokens WHERE token_hash = $1)")
                .bind(&token_hash)
                .fetch_one(&state.db)
                .await
                .map_err(|e| AppError::Internal(e.to_string()))?;

        if !exists {
            return Err(AppError::Unauthorized);
        }

        // Fire-and-forget: update last_used_at for auditing
        let db = state.db.clone();
        let hash = token_hash.clone();
        tokio::spawn(async move {
            if let Err(e) =
                sqlx::query("UPDATE service_tokens SET last_used_at = NOW() WHERE token_hash = $1")
                    .bind(&hash)
                    .execute(&db)
                    .await
            {
                tracing::warn!("failed to update service token last_used_at: {e}");
            }
        });

        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_token_is_used_for_lookup() {
        // ServiceAuth uses the same hash_token as AuthUser
        let hash = hash_token("test-service-token");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
