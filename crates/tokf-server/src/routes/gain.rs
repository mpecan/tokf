use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use sqlx::PgPool;

use crate::auth::token::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

// ── Response types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct MachineGain {
    pub machine_id: String,
    pub hostname: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub total_raw_tokens: i64,
}

/// Machine gain entry for the public global endpoint — hostname is omitted to
/// avoid leaking internal infrastructure details to unauthenticated callers.
#[derive(Debug, Serialize)]
pub struct GlobalMachineGain {
    pub machine_id: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub total_raw_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct FilterGainEntry {
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub total_raw_tokens: i64,
}

#[derive(Debug, Serialize)]
pub struct GainResponse {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub total_raw_tokens: i64,
    pub by_machine: Vec<MachineGain>,
    pub by_filter: Vec<FilterGainEntry>,
}

#[derive(Debug, Serialize)]
pub struct GlobalGainResponse {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub total_raw_tokens: i64,
    pub by_machine: Vec<GlobalMachineGain>,
    pub by_filter: Vec<FilterGainEntry>,
}

#[derive(Debug, Serialize)]
pub struct FilterStatsResponse {
    pub filter_hash: String,
    pub command_pattern: Option<String>,
    pub total_commands: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub savings_pct: f64,
    pub total_raw_tokens: i64,
    pub last_updated: String,
}

// ── Internal row types ─────────────────────────────────────────────────────────

type TotalsRow = (i64, i64, i64, i64);
type MachineRow = (String, String, i64, i64, i64, i64);
type GlobalMachineRow = (String, i64, i64, i64, i64);
type FilterRow = (Option<String>, Option<String>, i64, i64, i64, i64);
type FilterStatsRow = (
    String,
    Option<String>,
    i64,
    i64,
    i64,
    f64,
    i64,
    chrono::DateTime<chrono::Utc>,
);

// ── DB helpers ────────────────────────────────────────────────────────────────

async fn fetch_user_totals(pool: &PgPool, user_id: i64) -> Result<TotalsRow, AppError> {
    sqlx::query_as(
        "SELECT COALESCE(SUM(ue.input_tokens)::INT8, 0),
                COALESCE(SUM(ue.output_tokens)::INT8, 0),
                COALESCE(SUM(ue.command_count)::INT8, 0),
                COALESCE(SUM(ue.raw_tokens)::INT8, 0)
         FROM usage_events ue
         JOIN machines m ON ue.machine_id = m.id
         WHERE m.user_id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_user_by_machine(pool: &PgPool, user_id: i64) -> Result<Vec<MachineRow>, AppError> {
    sqlx::query_as(
        "SELECT m.id::TEXT, m.hostname,
                COALESCE(SUM(ue.input_tokens)::INT8, 0),
                COALESCE(SUM(ue.output_tokens)::INT8, 0),
                COALESCE(SUM(ue.command_count)::INT8, 0),
                COALESCE(SUM(ue.raw_tokens)::INT8, 0)
         FROM machines m
         LEFT JOIN usage_events ue ON ue.machine_id = m.id
         WHERE m.user_id = $1
         GROUP BY m.id, m.hostname
         ORDER BY COALESCE(SUM(ue.input_tokens)::INT8, 0) DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_user_by_filter(pool: &PgPool, user_id: i64) -> Result<Vec<FilterRow>, AppError> {
    sqlx::query_as(
        "SELECT ue.filter_name, ue.filter_hash,
                COALESCE(SUM(ue.input_tokens)::INT8, 0),
                COALESCE(SUM(ue.output_tokens)::INT8, 0),
                COALESCE(SUM(ue.command_count)::INT8, 0),
                COALESCE(SUM(ue.raw_tokens)::INT8, 0)
         FROM usage_events ue
         JOIN machines m ON ue.machine_id = m.id
         WHERE m.user_id = $1
         GROUP BY ue.filter_name, ue.filter_hash
         ORDER BY COALESCE(SUM(ue.input_tokens)::INT8, 0) DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_global_totals(pool: &PgPool) -> Result<TotalsRow, AppError> {
    sqlx::query_as(
        "SELECT COALESCE(SUM(input_tokens)::INT8, 0),
                COALESCE(SUM(output_tokens)::INT8, 0),
                COALESCE(SUM(command_count)::INT8, 0),
                COALESCE(SUM(raw_tokens)::INT8, 0)
         FROM usage_events",
    )
    .fetch_one(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_global_by_machine(pool: &PgPool) -> Result<Vec<GlobalMachineRow>, AppError> {
    sqlx::query_as(
        "SELECT m.id::TEXT,
                COALESCE(SUM(ue.input_tokens)::INT8, 0),
                COALESCE(SUM(ue.output_tokens)::INT8, 0),
                COALESCE(SUM(ue.command_count)::INT8, 0),
                COALESCE(SUM(ue.raw_tokens)::INT8, 0)
         FROM machines m
         LEFT JOIN usage_events ue ON ue.machine_id = m.id
         GROUP BY m.id
         ORDER BY COALESCE(SUM(ue.input_tokens)::INT8, 0) DESC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

async fn fetch_global_by_filter(pool: &PgPool) -> Result<Vec<FilterRow>, AppError> {
    sqlx::query_as(
        "SELECT filter_name, filter_hash,
                COALESCE(SUM(input_tokens)::INT8, 0),
                COALESCE(SUM(output_tokens)::INT8, 0),
                COALESCE(SUM(command_count)::INT8, 0),
                COALESCE(SUM(raw_tokens)::INT8, 0)
         FROM usage_events
         GROUP BY filter_name, filter_hash
         ORDER BY COALESCE(SUM(input_tokens)::INT8, 0) DESC
         LIMIT 100",
    )
    .fetch_all(pool)
    .await
    .map_err(AppError::from)
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn machine_rows_to_gains(rows: Vec<MachineRow>) -> Vec<MachineGain> {
    rows.into_iter()
        .map(
            |(machine_id, hostname, input, output, commands, raw)| MachineGain {
                machine_id,
                hostname,
                total_input_tokens: input,
                total_output_tokens: output,
                total_commands: commands,
                total_raw_tokens: raw,
            },
        )
        .collect()
}

fn global_machine_rows_to_gains(rows: Vec<GlobalMachineRow>) -> Vec<GlobalMachineGain> {
    rows.into_iter()
        .map(
            |(machine_id, input, output, commands, raw)| GlobalMachineGain {
                machine_id,
                total_input_tokens: input,
                total_output_tokens: output,
                total_commands: commands,
                total_raw_tokens: raw,
            },
        )
        .collect()
}

fn filter_rows_to_entries(rows: Vec<FilterRow>) -> Vec<FilterGainEntry> {
    rows.into_iter()
        .map(
            |(filter_name, filter_hash, input, output, commands, raw)| FilterGainEntry {
                filter_name,
                filter_hash,
                total_input_tokens: input,
                total_output_tokens: output,
                total_commands: commands,
                total_raw_tokens: raw,
            },
        )
        .collect()
}

fn build_gain_response(
    totals: TotalsRow,
    by_machine: Vec<MachineRow>,
    by_filter: Vec<FilterRow>,
) -> GainResponse {
    GainResponse {
        total_input_tokens: totals.0,
        total_output_tokens: totals.1,
        total_commands: totals.2,
        total_raw_tokens: totals.3,
        by_machine: machine_rows_to_gains(by_machine),
        by_filter: filter_rows_to_entries(by_filter),
    }
}

// ── Route handlers ────────────────────────────────────────────────────────────

/// GET /api/gain — authenticated user's own token savings
pub async fn get_gain(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<GainResponse>, AppError> {
    let totals = fetch_user_totals(&state.db, auth.user_id).await?;
    let by_machine = fetch_user_by_machine(&state.db, auth.user_id).await?;
    let by_filter = fetch_user_by_filter(&state.db, auth.user_id).await?;
    Ok(Json(build_gain_response(totals, by_machine, by_filter)))
}

/// GET /api/gain/global — public global token savings across all users.
/// Hostname is omitted from the machine breakdown to protect internal infrastructure.
/// Results are capped at the top 100 machines and filters by token savings.
pub async fn get_global_gain(
    State(state): State<AppState>,
) -> Result<Json<GlobalGainResponse>, AppError> {
    let totals = fetch_global_totals(&state.db).await?;
    let by_machine = fetch_global_by_machine(&state.db).await?;
    let by_filter = fetch_global_by_filter(&state.db).await?;
    Ok(Json(GlobalGainResponse {
        total_input_tokens: totals.0,
        total_output_tokens: totals.1,
        total_commands: totals.2,
        total_raw_tokens: totals.3,
        by_machine: global_machine_rows_to_gains(by_machine),
        by_filter: filter_rows_to_entries(by_filter),
    }))
}

/// GET /api/gain/filter/{hash} — public per-filter statistics
pub async fn get_filter_gain(
    Path(hash): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<FilterStatsResponse>, AppError> {
    let row: Option<FilterStatsRow> = sqlx::query_as(
        "SELECT fs.filter_hash, f.command_pattern,
                fs.total_commands, fs.total_input_tokens, fs.total_output_tokens,
                fs.savings_pct, fs.total_raw_tokens, fs.last_updated
         FROM filter_stats fs
         LEFT JOIN filters f ON fs.filter_hash = f.content_hash
         WHERE fs.filter_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(&state.db)
    .await?;

    match row {
        None => Err(AppError::NotFound(format!("filter {hash} not found"))),
        Some((
            filter_hash,
            command_pattern,
            total_commands,
            total_input_tokens,
            total_output_tokens,
            savings_pct,
            total_raw_tokens,
            last_updated,
        )) => Ok(Json(FilterStatsResponse {
            filter_hash,
            command_pattern,
            total_commands,
            total_input_tokens,
            total_output_tokens,
            savings_pct,
            total_raw_tokens,
            last_updated: last_updated.to_rfc3339(),
        })),
    }
}

// Tests live in a sibling file to keep this file within the 500-line soft limit.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "gain_tests.rs"]
mod tests;
