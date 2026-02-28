use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

use crate::rate_limit::RateLimitResult;

#[derive(Debug)]
pub enum AppError {
    Internal(String),
    BadRequest(String),
    NotFound(String),
    Forbidden(String),
    Conflict(String),
    RateLimited {
        retry_after_secs: u64,
        limit: u32,
        remaining: u32,
    },
    Unauthorized,
}

impl AppError {
    /// Construct a `RateLimited` error from a denied [`RateLimitResult`].
    pub const fn rate_limited(result: &RateLimitResult) -> Self {
        Self::RateLimited {
            retry_after_secs: result.reset_after_secs,
            limit: result.limit,
            remaining: result.remaining,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
            Self::BadRequest(msg) => write!(f, "bad request: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Forbidden(msg) => write!(f, "forbidden: {msg}"),
            Self::Conflict(msg) => write!(f, "conflict: {msg}"),
            Self::RateLimited { .. } => write!(f, "rate limited"),
            Self::Unauthorized => write!(f, "unauthorized"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    #[allow(clippy::unwrap_used)]
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::RateLimited {
                retry_after_secs,
                limit,
                remaining,
            } => {
                let mut headers = axum::http::HeaderMap::new();
                headers.insert("retry-after", retry_after_secs.to_string().parse().unwrap());
                headers.insert("x-ratelimit-limit", limit.to_string().parse().unwrap());
                headers.insert(
                    "x-ratelimit-remaining",
                    remaining.to_string().parse().unwrap(),
                );
                headers.insert(
                    "x-ratelimit-reset",
                    retry_after_secs.to_string().parse().unwrap(),
                );
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    headers,
                    Json(json!({ "error": "rate limit exceeded" })),
                )
                    .into_response()
            }
            Self::Internal(msg) => {
                tracing::error!("internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "internal server error" })),
                )
                    .into_response()
            }
            Self::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response()
            }
            Self::NotFound(msg) => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": msg }))).into_response()
            }
            Self::Forbidden(msg) => {
                (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response()
            }
            Self::Conflict(msg) => {
                (StatusCode::CONFLICT, Json(json!({ "error": msg }))).into_response()
            }
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response(),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn internal_error_returns_500() {
        let resp = AppError::Internal("db down".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "internal server error");
    }

    #[tokio::test]
    async fn bad_request_returns_400() {
        let resp = AppError::BadRequest("missing field".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "missing field");
    }

    #[tokio::test]
    async fn not_found_returns_404() {
        let resp = AppError::NotFound("no such flow".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn conflict_returns_409() {
        let resp = AppError::Conflict("already exists".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "already exists");
    }

    #[tokio::test]
    async fn rate_limited_returns_429_with_retry_after() {
        let resp = AppError::RateLimited {
            retry_after_secs: 3600,
            limit: 20,
            remaining: 0,
        }
        .into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
            Some("3600")
        );
    }

    #[tokio::test]
    async fn rate_limited_includes_all_headers() {
        let resp = AppError::RateLimited {
            retry_after_secs: 120,
            limit: 60,
            remaining: 0,
        }
        .into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers().get("retry-after").unwrap().to_str().unwrap(),
            "120"
        );
        assert_eq!(
            resp.headers()
                .get("x-ratelimit-limit")
                .unwrap()
                .to_str()
                .unwrap(),
            "60"
        );
        assert_eq!(
            resp.headers()
                .get("x-ratelimit-remaining")
                .unwrap()
                .to_str()
                .unwrap(),
            "0"
        );
        assert_eq!(
            resp.headers()
                .get("x-ratelimit-reset")
                .unwrap()
                .to_str()
                .unwrap(),
            "120"
        );
    }

    #[tokio::test]
    async fn forbidden_returns_403() {
        let resp = AppError::Forbidden("not the author".to_string()).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "not the author");
    }

    #[tokio::test]
    async fn unauthorized_returns_401() {
        let resp = AppError::Unauthorized.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
