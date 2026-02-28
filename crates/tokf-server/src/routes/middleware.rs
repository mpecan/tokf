use axum::{
    body::Body,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::AppError;
use crate::state::AppState;

/// General rate-limit middleware for authenticated requests.
///
/// Checks the bearer token against the general rate limiter (300/min per token).
/// Different tokens for the same user get independent counters — see
/// [`token_to_rate_limit_key`]. Adds `X-RateLimit-*` headers to the response
/// only if the handler did not already set them (endpoint-specific headers take
/// priority).
///
/// Unauthenticated requests (no `Authorization` header) pass through unmodified.
pub async fn general_rate_limit(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // Extract and hash the token before moving `request` into `next.run()`.
    let key = extract_bearer(request.headers()).map(token_to_rate_limit_key);
    match key {
        None => next.run(request).await,
        Some(key) => apply_general_limit(&state, key, request, next).await,
    }
}

fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
}

async fn apply_general_limit(
    state: &AppState,
    key: i64,
    request: Request<Body>,
    next: Next,
) -> Response {
    let rl = state.general_rate_limiter.check_and_increment(key);
    if !rl.allowed {
        return AppError::rate_limited(&rl).into_response();
    }
    let mut response = next.run(request).await;
    // Only add headers if the handler didn't already set endpoint-specific ones.
    if !response.headers().contains_key("x-ratelimit-limit") {
        let rl_headers = super::ip::rate_limit_headers(&rl);
        response.headers_mut().extend(rl_headers);
    }
    response
}

/// Hash a bearer token to a stable i64 key for rate-limiting.
///
/// Uses `DefaultHasher` to avoid a DB lookup in middleware. Different tokens
/// for the same user get independent counters — acceptable for single-instance
/// deployment where most users have 1-2 active tokens.
fn token_to_rate_limit_key(token: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish().cast_signed()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use axum::{Json, Router, body::Body, http::Request, middleware, routing::get};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    use crate::rate_limit::PublishRateLimiter;
    use crate::routes::test_helpers::make_state;

    use super::*;

    /// Minimal handler that does NOT set rate-limit headers.
    async fn hello() -> Json<serde_json::Value> {
        Json(json!({ "msg": "hello" }))
    }

    /// Handler that sets its own rate-limit headers.
    async fn custom_headers() -> (axum::http::HeaderMap, Json<serde_json::Value>) {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-ratelimit-limit", "5".parse().unwrap());
        headers.insert("x-ratelimit-remaining", "3".parse().unwrap());
        headers.insert("x-ratelimit-reset", "42".parse().unwrap());
        (headers, Json(json!({ "msg": "custom" })))
    }

    fn make_app(state: AppState) -> Router {
        Router::new()
            .route("/hello", get(hello))
            .route("/custom", get(custom_headers))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                general_rate_limit,
            ))
            .with_state(state)
    }

    fn authed_get(uri: &str, token: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn adds_headers_on_success() {
        let state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        let app = make_app(state);
        let resp = app.oneshot(authed_get("/hello", "tok_abc")).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        assert!(resp.headers().contains_key("x-ratelimit-limit"));
        assert!(resp.headers().contains_key("x-ratelimit-remaining"));
        assert!(resp.headers().contains_key("x-ratelimit-reset"));
    }

    #[tokio::test]
    async fn skips_unauthenticated_requests() {
        let state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        let app = make_app(state);
        let resp = app
            .oneshot(Request::get("/hello").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        assert!(!resp.headers().contains_key("x-ratelimit-limit"));
    }

    #[tokio::test]
    async fn returns_429_after_exceeding_limit() {
        let mut state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        state.general_rate_limiter = std::sync::Arc::new(PublishRateLimiter::new(1, 60));

        let app = make_app(state);

        let resp = app
            .clone()
            .oneshot(authed_get("/hello", "tok_limited"))
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        let resp = app
            .oneshot(authed_get("/hello", "tok_limited"))
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().contains_key("retry-after"));
        assert!(resp.headers().contains_key("x-ratelimit-limit"));
    }

    #[tokio::test]
    async fn does_not_override_handler_headers() {
        let state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        let app = make_app(state);
        let resp = app
            .oneshot(authed_get("/custom", "tok_custom"))
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        // Handler set x-ratelimit-limit=5, middleware should not override
        assert_eq!(
            resp.headers()
                .get("x-ratelimit-limit")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
        assert_eq!(
            resp.headers()
                .get("x-ratelimit-remaining")
                .unwrap()
                .to_str()
                .unwrap(),
            "3"
        );
    }

    #[tokio::test]
    async fn different_tokens_are_independent() {
        let mut state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        state.general_rate_limiter = std::sync::Arc::new(PublishRateLimiter::new(1, 60));

        let app = make_app(state);

        // Token A uses its quota
        let resp = app
            .clone()
            .oneshot(authed_get("/hello", "tok_a"))
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        // Token A is now blocked
        let resp = app
            .clone()
            .oneshot(authed_get("/hello", "tok_a"))
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);

        // Token B still has its own quota
        let resp = app.oneshot(authed_get("/hello", "tok_b")).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn body_preserved_on_429() {
        let mut state = make_state(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://localhost/fake")
                .unwrap(),
        );
        state.general_rate_limiter = std::sync::Arc::new(PublishRateLimiter::new(0, 60));

        let app = make_app(state);
        let resp = app.oneshot(authed_get("/hello", "tok_zero")).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "rate limit exceeded");
    }
}
