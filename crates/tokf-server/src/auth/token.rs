use axum::{extract::FromRequestParts, http::request::Parts};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::error::AppError;

/// Generates a cryptographically random bearer token (64 hex chars = 32 bytes).
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Returns the SHA-256 hex digest of the given token.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Authenticated user extracted from `Authorization: Bearer <token>`.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub username: String,
}

impl FromRequestParts<crate::state::AppState> for AuthUser {
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

        lookup_user_by_token_hash(&state.db, &token_hash).await
    }
}

async fn lookup_user_by_token_hash(db: &PgPool, token_hash: &str) -> Result<AuthUser, AppError> {
    let row = sqlx::query_as::<_, (i64, String, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT u.id, u.username, t.expires_at
         FROM auth_tokens t
         JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .ok_or(AppError::Unauthorized)?;

    let (user_id, username, expires_at) = row;

    if let Some(exp) = expires_at.filter(|&exp| exp < chrono::Utc::now()) {
        tracing::debug!(expires_at = %exp, "token expired");
        return Err(AppError::Unauthorized);
    }

    // Fire-and-forget: update last_used_at
    let db = db.clone();
    let hash = token_hash.to_string();
    tokio::spawn(async move {
        if let Err(e) =
            sqlx::query("UPDATE auth_tokens SET last_used_at = NOW() WHERE token_hash = $1")
                .bind(&hash)
                .execute(&db)
                .await
        {
            tracing::warn!("failed to update last_used_at: {e}");
        }
    });

    Ok(AuthUser { user_id, username })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn generate_token_returns_64_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
    }

    #[test]
    fn hash_token_is_deterministic() {
        let token = "test-token-123";
        assert_eq!(hash_token(token), hash_token(token));
    }

    #[test]
    fn hash_token_returns_64_hex_chars() {
        let hash = hash_token("anything");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_tokens_produce_different_hashes() {
        assert_ne!(hash_token("token-a"), hash_token("token-b"));
    }
}
