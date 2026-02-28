use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::auth::github::{AccessTokenResponse, GitHubClient, GitHubUser};
use crate::auth::token::{generate_token, hash_token};
use crate::error::AppError;
use crate::state::AppState;

const MAX_FLOWS_PER_IP_PER_HOUR: i64 = 10;
const TOKEN_TTL_SECONDS: i64 = 7_776_000; // 90 days

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Deserialize)]
pub struct PollTokenRequest {
    pub device_code: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: TokenUser,
}

#[derive(Debug, Serialize)]
pub struct TokenUser {
    pub id: i64,
    pub username: String,
    pub avatar_url: String,
}

#[derive(Debug, Serialize)]
pub struct PendingResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<i64>,
}

// ── POST /api/auth/device ───────────────────────────────────────────────────

/// Starts the GitHub device authorization flow.
///
/// # Errors
///
/// Returns `RateLimited` if the IP has exceeded 10 requests/hour, or
/// `Internal` on GitHub API or database failures.
pub async fn initiate_device_flow(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<(StatusCode, Json<DeviceFlowResponse>), AppError> {
    let ip = super::ip::extract_ip(&headers, state.trust_proxy, None);

    // Piggyback cleanup of expired flows
    let _ = sqlx::query("DELETE FROM device_flows WHERE expires_at < NOW()")
        .execute(&state.db)
        .await;

    // Rate limit check
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM device_flows
         WHERE ip_address = $1 AND created_at > NOW() - INTERVAL '1 hour'",
    )
    .bind(&ip)
    .fetch_one(&state.db)
    .await?;

    if count >= MAX_FLOWS_PER_IP_PER_HOUR {
        return Err(AppError::RateLimited {
            retry_after_secs: 3600,
            limit: 10,
            remaining: 0,
        });
    }

    let gh_resp = state
        .github
        .request_device_code(&state.github_client_id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Store the device flow in DB
    let interval_secs = i32::try_from(gh_resp.interval)
        .map_err(|_| AppError::Internal("interval out of range".to_string()))?;
    sqlx::query(
        "INSERT INTO device_flows (device_code, user_code, verification_uri, interval_secs, ip_address, expires_at)
         VALUES ($1, $2, $3, $4, $5, NOW() + $6 * INTERVAL '1 second')",
    )
    .bind(&gh_resp.device_code)
    .bind(&gh_resp.user_code)
    .bind(&gh_resp.verification_uri)
    .bind(interval_secs)
    .bind(&ip)
    .bind(gh_resp.expires_in)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(DeviceFlowResponse {
            device_code: gh_resp.device_code,
            user_code: gh_resp.user_code,
            verification_uri: gh_resp.verification_uri,
            expires_in: gh_resp.expires_in,
            interval: gh_resp.interval,
        }),
    ))
}

/// Polls for a completed device authorization and exchanges it for a bearer token.
///
/// # Errors
///
/// Returns `NotFound` for unknown device codes, `BadRequest` for denied or
/// expired codes, or `Internal` on GitHub API / database failures.
/// Re-polling a completed code is idempotent: a fresh token is issued for the
/// same user.
pub async fn poll_token(
    State(state): State<AppState>,
    Json(req): Json<PollTokenRequest>,
) -> Result<axum::response::Response, AppError> {
    let flow = sqlx::query_as::<_, (i64, String, Option<i64>)>(
        "SELECT id, status, user_id FROM device_flows WHERE device_code = $1",
    )
    .bind(&req.device_code)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("unknown device code".to_string()))?;

    let (flow_id, status, flow_user_id) = flow;

    // Idempotent: if already completed, issue a fresh token for the same user
    if status == "completed" {
        return match flow_user_id {
            Some(uid) => build_token_response(&state, uid).await,
            None => Err(AppError::BadRequest("device code already used".to_string())),
        };
    }

    // Poll GitHub for access token
    let gh_resp = state
        .github
        .poll_access_token(
            &state.github_client_id,
            &state.github_client_secret,
            &req.device_code,
        )
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    match gh_resp {
        AccessTokenResponse::Pending {
            error, interval, ..
        } => handle_pending_response(&state, flow_id, &error, interval).await,
        AccessTokenResponse::Success { access_token, .. } => {
            handle_github_success(&state, flow_id, &access_token).await
        }
    }
}

// ── Internal helpers ────────────────────────────────────────────────────────

async fn handle_pending_response(
    state: &AppState,
    flow_id: i64,
    error: &str,
    interval: Option<i64>,
) -> Result<axum::response::Response, AppError> {
    match error {
        "authorization_pending" => {
            let body = PendingResponse {
                error: error.to_string(),
                interval: None,
            };
            Ok(axum::response::IntoResponse::into_response((
                StatusCode::OK,
                Json(body),
            )))
        }
        "slow_down" => {
            let body = PendingResponse {
                error: error.to_string(),
                interval,
            };
            Ok(axum::response::IntoResponse::into_response((
                StatusCode::OK,
                Json(body),
            )))
        }
        "expired_token" => {
            let _ = sqlx::query("UPDATE device_flows SET status = 'expired' WHERE id = $1")
                .bind(flow_id)
                .execute(&state.db)
                .await;
            Err(AppError::BadRequest("device code expired".to_string()))
        }
        "access_denied" => {
            let _ = sqlx::query("UPDATE device_flows SET status = 'denied' WHERE id = $1")
                .bind(flow_id)
                .execute(&state.db)
                .await;
            Err(AppError::BadRequest("access denied by user".to_string()))
        }
        _ => Err(AppError::Internal(format!(
            "unexpected GitHub error: {error}"
        ))),
    }
}

async fn handle_github_success(
    state: &AppState,
    flow_id: i64,
    access_token: &str,
) -> Result<axum::response::Response, AppError> {
    let (user, orgs) = fetch_github_profile(&*state.github, access_token).await?;
    let user_id = upsert_github_user(state, &user, &orgs).await?;
    let (bearer, expires_in) = create_bearer_token(state, user_id).await?;

    // Atomic CAS: only mark completed if still pending (prevents races)
    let result = sqlx::query(
        "UPDATE device_flows SET status = 'completed', user_id = $1, completed_at = NOW()
         WHERE id = $2 AND status = 'pending'",
    )
    .bind(user_id)
    .bind(flow_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest("device code already used".to_string()));
    }

    let resp = TokenResponse {
        access_token: bearer,
        token_type: "bearer".to_string(),
        expires_in,
        user: TokenUser {
            id: user_id,
            username: user.login,
            avatar_url: user.avatar_url,
        },
    };
    Ok(axum::response::IntoResponse::into_response((
        StatusCode::OK,
        Json(resp),
    )))
}

/// Look up a user by ID and issue a fresh bearer token.
async fn build_token_response(
    state: &AppState,
    user_id: i64,
) -> Result<axum::response::Response, AppError> {
    let (username, avatar_url): (String, String) =
        sqlx::query_as("SELECT username, avatar_url FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;

    let (bearer, expires_in) = create_bearer_token(state, user_id).await?;

    let resp = TokenResponse {
        access_token: bearer,
        token_type: "bearer".to_string(),
        expires_in,
        user: TokenUser {
            id: user_id,
            username,
            avatar_url,
        },
    };
    Ok(axum::response::IntoResponse::into_response((
        StatusCode::OK,
        Json(resp),
    )))
}

async fn upsert_github_user(
    state: &AppState,
    user: &GitHubUser,
    orgs: &[crate::auth::github::GitHubOrg],
) -> Result<i64, AppError> {
    let org_logins: Vec<&str> = orgs.iter().map(|o| o.login.as_str()).collect();
    let orgs_json =
        serde_json::to_value(&org_logins).map_err(|e| AppError::Internal(e.to_string()))?;

    let user_id: i64 = sqlx::query_scalar(
        "INSERT INTO users (github_id, username, avatar_url, profile_url, orgs)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (github_id) DO UPDATE SET
             username = EXCLUDED.username,
             avatar_url = EXCLUDED.avatar_url,
             profile_url = EXCLUDED.profile_url,
             orgs = EXCLUDED.orgs,
             updated_at = NOW()
         RETURNING id",
    )
    .bind(user.id)
    .bind(&user.login)
    .bind(&user.avatar_url)
    .bind(&user.html_url)
    .bind(&orgs_json)
    .fetch_one(&state.db)
    .await?;

    Ok(user_id)
}

/// Generate and store a bearer token with a 90-day TTL.
async fn create_bearer_token(state: &AppState, user_id: i64) -> Result<(String, i64), AppError> {
    let bearer = generate_token();
    let bearer_hash = hash_token(&bearer);

    sqlx::query(
        "INSERT INTO auth_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, NOW() + INTERVAL '90 days')",
    )
    .bind(user_id)
    .bind(&bearer_hash)
    .execute(&state.db)
    .await?;

    Ok((bearer, TOKEN_TTL_SECONDS))
}

async fn fetch_github_profile(
    github: &dyn GitHubClient,
    access_token: &str,
) -> Result<(GitHubUser, Vec<crate::auth::github::GitHubOrg>), AppError> {
    let user = github
        .get_user(access_token)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let orgs = github
        .get_user_orgs(access_token)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok((user, orgs))
}
