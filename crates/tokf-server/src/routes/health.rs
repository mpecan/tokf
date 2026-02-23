use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

/// Liveness probe: always returns 200 while the process is running.
///
/// This endpoint never queries the database, so it remains responsive even
/// when the DB is unavailable. Kubernetes should use this for the liveness
/// probe so pods are not restarted due to a DB outage.
///
/// For a readiness check that verifies DB connectivity, use `GET /ready`.
pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use std::sync::Arc;

    use crate::auth::mock::NoOpGitHubClient;
    use crate::state::AppState;

    fn test_state() -> AppState {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tokf:tokf@localhost:5432/tokf_dev".to_string());
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy(&url)
            .expect("invalid DATABASE_URL");
        AppState {
            db: pool,
            github: Arc::new(NoOpGitHubClient),
            github_client_id: "test-client-id".to_string(),
            github_client_secret: "test-client-secret".to_string(),
            trust_proxy: true,
        }
    }

    #[tokio::test]
    async fn health_always_returns_200() {
        let app = crate::routes::create_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to get response");

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_returns_status_and_version_fields() {
        let app = crate::routes::create_router(test_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to get response");

        let body = resp
            .into_body()
            .collect()
            .await
            .expect("failed to collect body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("failed to parse JSON");
        assert_eq!(json["status"], "ok", "status should always be ok");
        assert!(
            json["version"].is_string(),
            "version field should be present"
        );
        assert!(
            json["database"].is_null(),
            "database field should not be present"
        );
    }
}
