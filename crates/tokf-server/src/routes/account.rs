use axum::extract::State;
use axum::http::StatusCode;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

/// Delete the authenticated user's account.
///
/// Anonymizes the user row (clears personal data, sets `visible = false`,
/// marks `deleted_at`) so filter `author_id` references remain valid.
/// Cascades deletion of auth tokens, machines (and their usage events /
/// sync cursors), device flows, and `ToS` acceptance records.
///
/// Returns `204 No Content` on success.
pub async fn delete_account(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<StatusCode, AppError> {
    // Run each statement independently (no wrapping transaction).
    // CockroachDB's serializable isolation causes WriteTooOldError when
    // a transaction DELETEs auth_tokens that were just read by the
    // AuthUser extractor. Individual statements avoid this conflict and
    // are safe here: each targets a different table, operations are
    // idempotent, and partial failure is recoverable by re-running.

    // Anonymize the user row first — this is the point of no return.
    sqlx::query(
        "UPDATE users SET
            username = CONCAT('deleted-user-', id::TEXT),
            avatar_url = '',
            profile_url = '',
            orgs = '[]'::jsonb,
            visible = false,
            deleted_at = NOW(),
            updated_at = NOW()
         WHERE id = $1",
    )
    .bind(user.user_id)
    .execute(&state.db)
    .await?;

    // Delete cascading data that references user_id
    sqlx::query("DELETE FROM tos_acceptances WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&state.db)
        .await?;

    sqlx::query("DELETE FROM auth_tokens WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&state.db)
        .await?;

    // usage_events.machine_id has no ON DELETE CASCADE, so delete explicitly.
    // sync_cursors does cascade from machines.
    sqlx::query(
        "DELETE FROM usage_events WHERE machine_id IN
             (SELECT id FROM machines WHERE user_id = $1)",
    )
    .bind(user.user_id)
    .execute(&state.db)
    .await?;

    sqlx::query("DELETE FROM machines WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&state.db)
        .await?;

    sqlx::query("DELETE FROM device_flows WHERE user_id = $1")
        .bind(user.user_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::delete;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::routes::test_helpers::*;

    use super::*;

    fn app(pool: PgPool) -> Router {
        Router::new()
            .route("/api/account", delete(delete_account))
            .with_state(make_state(pool))
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn delete_account_requires_auth(pool: PgPool) {
        let resp = app(pool)
            .oneshot(Request::delete("/api/account").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_status(resp, StatusCode::UNAUTHORIZED).await;
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn delete_account_anonymizes_user(pool: PgPool) {
        let (user_id, token) = create_user_and_token(&pool).await;

        let resp = app(pool.clone())
            .oneshot(
                Request::delete("/api/account")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::NO_CONTENT).await;

        // User row still exists but is anonymized
        let (username, visible, deleted_at): (String, bool, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as("SELECT username, visible, deleted_at FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            username.starts_with("deleted-user-"),
            "expected anonymized username, got: {username}"
        );
        assert!(!visible);
        assert!(deleted_at.is_some());

        // Auth tokens are gone
        let token_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM auth_tokens WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(token_count, 0);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn delete_account_preserves_filter_attribution(pool: PgPool) {
        let (user_id, token) = create_user_and_token(&pool).await;

        // Create a filter owned by this user
        sqlx::query(
            "INSERT INTO filters (content_hash, command_pattern, canonical_command, author_id, r2_key)
             VALUES ('hash123', 'test *', 'test', $1, 'r2/hash123')",
        )
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

        let resp = app(pool.clone())
            .oneshot(
                Request::delete("/api/account")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::NO_CONTENT).await;

        // Filter still exists and still references the user
        let author_id: i64 =
            sqlx::query_scalar("SELECT author_id FROM filters WHERE content_hash = 'hash123'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(author_id, user_id);
    }

    #[crdb_test_macro::crdb_test(migrations = "./migrations")]
    async fn delete_account_token_becomes_invalid(pool: PgPool) {
        let (_, token) = create_user_and_token(&pool).await;

        // Delete the account
        let resp = app(pool.clone())
            .oneshot(
                Request::delete("/api/account")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::NO_CONTENT).await;

        // Using the same token again should fail
        let resp = app(pool)
            .oneshot(
                Request::delete("/api/account")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_status(resp, StatusCode::UNAUTHORIZED).await;
    }
}
