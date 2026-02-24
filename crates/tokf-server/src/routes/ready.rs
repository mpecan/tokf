use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

use crate::state::AppState;

/// Readiness probe: returns 200 only when the database is reachable and
/// migrations have been applied. Returns 503 otherwise.
///
/// Kubernetes should route traffic only to pods that pass this check.
/// For a lighter liveness check that never queries the DB, use `GET /health`.
pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    // Querying _sqlx_migrations (rather than SELECT 1) confirms both
    // connectivity and that at least one migration has been applied.
    let db_ok = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&state.db)
        .await
        .is_ok();

    let (status, db_status) = if db_ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "error")
    };
    (
        status,
        Json(json!({
            "status": if db_ok { "ok" } else { "degraded" },
            "version": env!("CARGO_PKG_VERSION"),
            "database": db_status
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
    use crate::storage::noop::NoOpStorageClient;

    /// Creates an `AppState` whose pool will always fail to acquire a connection.
    /// Uses a non-resolvable hostname (RFC 2606 `.invalid` TLD) so DNS returns
    /// NXDOMAIN immediately, and a short `acquire_timeout` to cap any delay.
    fn down_state() -> AppState {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(500))
            .connect_lazy("postgres://tokf:tokf@nonexistent-host.invalid:5432/tokf")
            .expect("lazy pool creation should not fail");
        AppState {
            db: pool,
            github: Arc::new(NoOpGitHubClient),
            storage: Arc::new(NoOpStorageClient),
            github_client_id: "test-client-id".to_string(),
            github_client_secret: "test-client-secret".to_string(),
            trust_proxy: true,
        }
    }

    #[tokio::test]
    async fn ready_returns_503_and_degraded_status_when_db_is_down() {
        let app = crate::routes::create_router(down_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to get response");

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("failed to collect body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("failed to parse JSON");
        assert_eq!(json["status"], "degraded", "status should be degraded");
        assert_eq!(json["database"], "error", "database should be error");
        assert!(
            json["version"].is_string(),
            "version field should be present"
        );
    }
}
