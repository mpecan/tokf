use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

const MAX_BATCH_SIZE: usize = 1000;
const MAX_TOKENS_PER_EVENT: i64 = 10_000_000;
const MAX_COMMAND_COUNT: i32 = 100_000;
const MAX_FILTER_NAME_LEN: usize = 1024;

#[derive(Debug, Deserialize)]
pub struct SyncEvent {
    pub id: i64,
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub command_count: i32,
    pub recorded_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SyncRequest {
    pub machine_id: String,
    /// Client-supplied hint; accepted for forward-compatibility but not consumed —
    /// the server always uses the authoritative DB cursor.
    #[allow(dead_code)] // part of the public API contract; intentionally not read
    pub last_event_id: i64,
    pub events: Vec<SyncEvent>,
}

#[derive(Debug, Serialize)]
pub struct SyncResponse {
    pub accepted: usize,
    pub cursor: i64,
}

async fn verify_machine_owner(
    pool: &PgPool,
    machine_id: Uuid,
    user_id: i64,
) -> Result<(), AppError> {
    let owner: Option<i64> = sqlx::query_scalar("SELECT user_id FROM machines WHERE id = $1")
        .bind(machine_id)
        .fetch_optional(pool)
        .await?;
    match owner {
        Some(uid) if uid == user_id => Ok(()),
        Some(_) => Err(AppError::Unauthorized),
        None => Err(AppError::NotFound("machine not found".to_string())),
    }
}

async fn fetch_cursor(pool: &PgPool, machine_id: Uuid) -> Result<i64, AppError> {
    let cursor: Option<i64> =
        sqlx::query_scalar("SELECT last_event_id FROM sync_cursors WHERE machine_id = $1")
            .bind(machine_id)
            .fetch_optional(pool)
            .await?;
    Ok(cursor.unwrap_or(0))
}

fn collect_unique_filter_hashes(events: &[&SyncEvent]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    events
        .iter()
        .filter_map(|e| e.filter_hash.as_deref())
        .filter(|h| seen.insert(*h))
        .map(ToString::to_string)
        .collect()
}

async fn persist_events(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    events: &[&SyncEvent],
    machine_id: Uuid,
    new_cursor: i64,
    filter_hashes: &[String],
) -> Result<(), AppError> {
    for event in events {
        let recorded_at = chrono::DateTime::parse_from_rfc3339(&event.recorded_at)
            .map_err(|_| {
                AppError::BadRequest(format!(
                    "event {}: invalid recorded_at timestamp (expected RFC 3339, e.g. 2026-01-01T00:00:00Z)",
                    event.id
                ))
            })?
            .with_timezone(&chrono::Utc);

        sqlx::query(
            "INSERT INTO usage_events
                (filter_hash, filter_name, machine_id, input_tokens, output_tokens, command_count, recorded_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(event.filter_hash.as_deref())
        .bind(event.filter_name.as_deref())
        .bind(machine_id)
        .bind(event.input_tokens)
        .bind(event.output_tokens)
        .bind(event.command_count)
        .bind(recorded_at)
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO sync_cursors (machine_id, last_event_id, synced_at)
         VALUES ($1, $2, NOW())
         ON CONFLICT (machine_id) DO UPDATE SET
             last_event_id = GREATEST(sync_cursors.last_event_id, EXCLUDED.last_event_id),
             synced_at = NOW()",
    )
    .bind(machine_id)
    .bind(new_cursor)
    .execute(&mut **tx)
    .await?;

    sqlx::query("UPDATE machines SET last_sync_at = NOW() WHERE id = $1")
        .bind(machine_id)
        .execute(&mut **tx)
        .await?;

    for hash in filter_hashes {
        update_filter_stats(tx, hash).await?;
    }

    Ok(())
}

async fn update_filter_stats(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    hash: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO filter_stats
            (filter_hash, total_commands, total_input_tokens, total_output_tokens, savings_pct, last_updated)
         SELECT
            filter_hash,
            SUM(command_count)::INT8,
            SUM(input_tokens)::INT8,
            SUM(output_tokens)::INT8,
            CASE WHEN SUM(input_tokens) > 0
                 THEN (SUM(input_tokens) - SUM(output_tokens))::FLOAT8 / SUM(input_tokens)::FLOAT8
                 ELSE 0.0 END,
            NOW()
         FROM usage_events WHERE filter_hash = $1
         GROUP BY filter_hash
         ON CONFLICT (filter_hash) DO UPDATE SET
             total_commands = EXCLUDED.total_commands,
             total_input_tokens = EXCLUDED.total_input_tokens,
             total_output_tokens = EXCLUDED.total_output_tokens,
             savings_pct = EXCLUDED.savings_pct,
             last_updated = EXCLUDED.last_updated",
    )
    .bind(hash)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn validate_events(events: &[SyncEvent]) -> Result<(), AppError> {
    for event in events {
        if event.input_tokens < 0 || event.input_tokens > MAX_TOKENS_PER_EVENT {
            return Err(AppError::BadRequest(format!(
                "event {}: input_tokens out of range [0, {MAX_TOKENS_PER_EVENT}]",
                event.id
            )));
        }
        if event.output_tokens < 0 || event.output_tokens > MAX_TOKENS_PER_EVENT {
            return Err(AppError::BadRequest(format!(
                "event {}: output_tokens out of range [0, {MAX_TOKENS_PER_EVENT}]",
                event.id
            )));
        }
        if event.command_count < 0 || event.command_count > MAX_COMMAND_COUNT {
            return Err(AppError::BadRequest(format!(
                "event {}: command_count out of range [0, {MAX_COMMAND_COUNT}]",
                event.id
            )));
        }
        if let Some(ref name) = event.filter_name
            && name.len() > MAX_FILTER_NAME_LEN
        {
            return Err(AppError::BadRequest(format!(
                "event {}: filter_name exceeds max length of {MAX_FILTER_NAME_LEN}",
                event.id
            )));
        }
    }
    Ok(())
}

pub async fn sync_usage(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<SyncRequest>,
) -> Result<(axum::http::HeaderMap, Json<SyncResponse>), AppError> {
    if req.events.len() > MAX_BATCH_SIZE {
        return Err(AppError::BadRequest(format!(
            "batch size {} exceeds limit of {MAX_BATCH_SIZE}",
            req.events.len()
        )));
    }

    validate_events(&req.events)?;

    let machine_id = Uuid::parse_str(&req.machine_id)
        .map_err(|_| AppError::BadRequest("invalid machine_id UUID".to_string()))?;

    verify_machine_owner(&state.db, machine_id, auth.user_id).await?;

    let rl = state
        .sync_rate_limiter
        .check_and_increment(machine_id.as_u128());
    if !rl.allowed {
        return Err(AppError::rate_limited(&rl));
    }
    let rl_headers = crate::routes::ip::rate_limit_headers(&rl);

    let cursor = fetch_cursor(&state.db, machine_id).await?;
    let new_events: Vec<&SyncEvent> = req.events.iter().filter(|e| e.id > cursor).collect();

    if new_events.is_empty() {
        return Ok((
            rl_headers,
            Json(SyncResponse {
                accepted: 0,
                cursor,
            }),
        ));
    }

    let new_cursor = new_events.iter().map(|e| e.id).max().unwrap_or(cursor);
    let accepted = new_events.len();
    let filter_hashes = collect_unique_filter_hashes(&new_events);

    let mut tx = state.db.begin().await?;
    persist_events(&mut tx, &new_events, machine_id, new_cursor, &filter_hashes).await?;
    tx.commit().await?;

    Ok((
        rl_headers,
        Json(SyncResponse {
            accepted,
            cursor: new_cursor,
        }),
    ))
}

// Tests live in a sibling file to keep this file within the 500-line soft limit.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "sync_tests.rs"]
mod tests;
