use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;
use crate::tos::{CURRENT_TOS_VERSION, TOS_CONTENT_MD};

// ── GET /terms ────────────────────────────────────────────────────────────────

/// Serves the full Terms of Service as Markdown text.
pub async fn get_terms() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/markdown; charset=utf-8")],
        TOS_CONTENT_MD,
    )
}

// ── GET /api/tos ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TosInfoResponse {
    pub version: i32,
    pub url: String,
}

/// Returns the current `ToS` version and a link to the full text.
pub async fn get_tos_info(State(state): State<AppState>) -> Json<TosInfoResponse> {
    Json(TosInfoResponse {
        version: CURRENT_TOS_VERSION,
        url: format!("{}/terms", state.public_url),
    })
}

// ── POST /api/tos/accept ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AcceptTosRequest {
    pub version: i32,
}

#[derive(Debug, Serialize)]
pub struct AcceptTosResponse {
    pub accepted_version: i32,
    pub accepted_at: String,
}

/// Records the authenticated user's acceptance of a specific `ToS` version.
///
/// # Errors
///
/// Returns `BadRequest` if the version doesn't match the current version,
/// or `Unauthorized` if not authenticated.
pub async fn accept_tos(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<AcceptTosRequest>,
) -> Result<Json<AcceptTosResponse>, AppError> {
    if req.version != CURRENT_TOS_VERSION {
        return Err(AppError::BadRequest(format!(
            "expected ToS version {CURRENT_TOS_VERSION}, got {}",
            req.version
        )));
    }

    let accepted_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "INSERT INTO tos_acceptances (user_id, tos_version) VALUES ($1, $2) RETURNING accepted_at",
    )
    .bind(user.user_id)
    .bind(req.version)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(AcceptTosResponse {
        accepted_version: req.version,
        accepted_at: accepted_at.to_rfc3339(),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::routes::test_helpers::*;
    use crate::tos::CURRENT_TOS_VERSION;

    use super::*;

    fn app(pool: PgPool) -> Router {
        Router::new()
            .route("/terms", get(get_terms))
            .route("/api/tos", get(get_tos_info))
            .route("/api/tos/accept", post(accept_tos))
            .with_state(make_state(pool))
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn get_terms_returns_markdown(pool: PgPool) {
        let resp = app(pool)
            .oneshot(Request::get("/terms").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("text/markdown"));
        let body = assert_status(resp, StatusCode::OK).await;
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("Terms of Service"));
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn get_tos_info_returns_version(pool: PgPool) {
        let resp = app(pool)
            .oneshot(Request::get("/api/tos").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = assert_status(resp, StatusCode::OK).await;
        let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(info["version"], CURRENT_TOS_VERSION);
        assert!(info["url"].as_str().unwrap().contains("/terms"));
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn accept_tos_requires_auth(pool: PgPool) {
        let resp = app(pool)
            .oneshot(
                Request::post("/api/tos/accept")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "version": CURRENT_TOS_VERSION }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::UNAUTHORIZED).await;
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn accept_tos_records_acceptance(pool: PgPool) {
        let (user_id, token) = create_user_and_token(&pool).await;
        let resp = app(pool.clone())
            .oneshot(
                Request::post("/api/tos/accept")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::json!({ "version": CURRENT_TOS_VERSION }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = assert_status(resp, StatusCode::OK).await;
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(result["accepted_version"], CURRENT_TOS_VERSION);

        // Verify DB record
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tos_acceptances WHERE user_id = $1 AND tos_version = $2",
        )
        .bind(user_id)
        .bind(CURRENT_TOS_VERSION)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn accept_tos_rejects_wrong_version(pool: PgPool) {
        let (_, token) = create_user_and_token(&pool).await;
        let resp = app(pool)
            .oneshot(
                Request::post("/api/tos/accept")
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        serde_json::json!({ "version": 999 }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::BAD_REQUEST).await;
    }
}
