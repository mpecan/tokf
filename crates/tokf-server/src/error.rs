use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    Internal(String),
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    RateLimited,
    Unauthorized,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
            Self::BadRequest(msg) => write!(f, "bad request: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Conflict(msg) => write!(f, "conflict: {msg}"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::Unauthorized => write!(f, "unauthorized"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "3600")],
                Json(json!({ "error": "rate limit exceeded" })),
            )
                .into_response(),
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
        let resp = AppError::RateLimited.into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            resp.headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()),
            Some("3600")
        );
    }

    #[tokio::test]
    async fn unauthorized_returns_401() {
        let resp = AppError::Unauthorized.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
