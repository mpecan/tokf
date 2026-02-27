use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rusqlite::{Connection, OptionalExtension as _};

pub use tokf_common::tracking::types::{DailyGain, FilterGain, GainSummary, TrackingEvent};

/// Returns the DB path: `TOKF_DB_PATH` env var overrides; else
/// `TOKF_HOME/tracking.db` if `TOKF_HOME` is set; else
/// `dirs::data_local_dir()/tokf/tracking.db`.
pub fn db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TOKF_DB_PATH") {
        return Some(PathBuf::from(p));
    }
    crate::paths::user_data_dir().map(|d| d.join("tracking.db"))
}

/// Open or create the DB at `path`, running `CREATE TABLE IF NOT EXISTS` for the
/// events table.
///
/// To also initialize the history table, use [`crate::history::open_db`] instead.
///
/// # Errors
/// Returns an error if the directory cannot be created or the DB cannot be opened.
pub fn open_db(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create db dir {}", parent.display()))?;
    }
    // Pre-flight: SQLite opens read-only files silently but fails on the first write (INSERT/CREATE).
    // Catch this early with a clear, actionable error that includes the path.
    if path.exists() {
        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .with_context(|| format!("cannot open DB file {} for writing", path.display()))?;
    }
    let conn = Connection::open(path).with_context(|| format!("open db at {}", path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT    NOT NULL,
            command           TEXT    NOT NULL,
            filter_name       TEXT,
            filter_hash       TEXT,
            input_bytes       INTEGER NOT NULL,
            output_bytes      INTEGER NOT NULL,
            input_tokens_est  INTEGER NOT NULL,
            output_tokens_est INTEGER NOT NULL,
            filter_time_ms    INTEGER NOT NULL,
            exit_code         INTEGER NOT NULL,
            pipe_override     INTEGER NOT NULL DEFAULT 0
        );",
    )
    .context("create events table")?;

    // Migration: add pipe_override column when upgrading from a schema without it.
    let has_pipe_override: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='pipe_override'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_pipe_override == 0 {
        conn.execute_batch(
            "ALTER TABLE events ADD COLUMN pipe_override INTEGER NOT NULL DEFAULT 0;",
        )
        .context("migrate events table: add pipe_override column")?;
    }

    // Migration: add filter_hash column when upgrading from a schema without it.
    let has_filter_hash: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name='filter_hash'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_filter_hash == 0 {
        conn.execute_batch("ALTER TABLE events ADD COLUMN filter_hash TEXT;")
            .context("migrate events table: add filter_hash column")?;
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )
    .context("create sync_state table")?;

    Ok(conn)
}

/// Pure constructor — no I/O. Computes token estimates from bytes.
#[allow(clippy::too_many_arguments)]
pub fn build_event(
    command: &str,
    filter_name: Option<&str>,
    filter_hash: Option<&str>,
    input_bytes: usize,
    output_bytes: usize,
    filter_time_ms: u128,
    exit_code: i32,
    pipe_override: bool,
) -> TrackingEvent {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let input_tokens_est = (input_bytes / 4) as i64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let output_tokens_est = (output_bytes / 4) as i64;
    #[allow(clippy::cast_possible_truncation)]
    let filter_time_ms_i64 = filter_time_ms.min(i64::MAX as u128) as i64;
    TrackingEvent {
        command: command.to_owned(),
        filter_name: filter_name.map(ToOwned::to_owned),
        filter_hash: filter_hash.map(ToOwned::to_owned),
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        input_bytes: input_bytes as i64,
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        output_bytes: output_bytes as i64,
        input_tokens_est,
        output_tokens_est,
        filter_time_ms: filter_time_ms_i64,
        exit_code,
        pipe_override,
    }
}

/// Insert one row; timestamp set by `SQLite` `strftime` in the SQL.
///
/// # Errors
/// Returns an error if the INSERT fails.
pub fn record_event(conn: &Connection, event: &TrackingEvent) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO events
            (timestamp, command, filter_name, filter_hash,
             input_bytes, output_bytes,
             input_tokens_est, output_tokens_est,
             filter_time_ms, exit_code, pipe_override)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            event.command,
            event.filter_name,
            event.filter_hash,
            event.input_bytes,
            event.output_bytes,
            event.input_tokens_est,
            event.output_tokens_est,
            event.filter_time_ms,
            event.exit_code,
            i64::from(event.pipe_override),
        ],
    )
    .context("insert event")?;
    Ok(())
}

/// # Errors
/// Returns an error if the SQL query fails.
pub fn query_summary(conn: &Connection) -> anyhow::Result<GainSummary> {
    let row = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(input_tokens_est),0),
                    COALESCE(SUM(output_tokens_est),0),
                    COALESCE(SUM(input_tokens_est - output_tokens_est),0),
                    COALESCE(SUM(pipe_override),0)
             FROM events",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .context("query summary")?;

    let (
        total_commands,
        total_input_tokens,
        total_output_tokens,
        tokens_saved,
        pipe_override_count,
    ) = row;
    let savings_pct = if total_input_tokens == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let pct = tokens_saved as f64 / total_input_tokens as f64 * 100.0;
        pct
    };

    Ok(GainSummary {
        total_commands,
        total_input_tokens,
        total_output_tokens,
        tokens_saved,
        savings_pct,
        pipe_override_count,
    })
}

/// Row type returned by aggregate queries.
type AggregateRow = (String, i64, i64, i64, i64, i64);

/// Shared row mapper for aggregate queries. Returns `(label, commands, input, output, saved, pipe_overrides)`.
fn map_aggregate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AggregateRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, i64>(3)?,
        row.get::<_, i64>(4)?,
        row.get::<_, i64>(5)?,
    ))
}

/// Compute savings percentage from input tokens and tokens saved.
#[allow(clippy::cast_precision_loss)]
fn savings_pct(input_tokens: i64, tokens_saved: i64) -> f64 {
    if input_tokens == 0 {
        0.0
    } else {
        tokens_saved as f64 / input_tokens as f64 * 100.0
    }
}

/// # Errors
/// Returns an error if the SQL query fails.
pub fn query_by_filter(conn: &Connection) -> anyhow::Result<Vec<FilterGain>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(filter_name, 'passthrough'), COUNT(*),
                SUM(input_tokens_est), SUM(output_tokens_est),
                SUM(input_tokens_est - output_tokens_est),
                COALESCE(SUM(pipe_override),0)
         FROM events
         GROUP BY filter_name
         ORDER BY SUM(input_tokens_est - output_tokens_est) DESC",
    )?;

    let rows = stmt.query_map([], map_aggregate_row)?;

    let mut result = Vec::new();
    for row in rows {
        let (filter_name, commands, input_tokens, output_tokens, tokens_saved, pipe_override_count) =
            row.context("read filter row")?;
        result.push(FilterGain {
            filter_name,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct: savings_pct(input_tokens, tokens_saved),
            pipe_override_count,
        });
    }
    Ok(result)
}

/// # Errors
/// Returns an error if the SQL query fails.
pub fn query_daily(conn: &Connection) -> anyhow::Result<Vec<DailyGain>> {
    let mut stmt = conn.prepare(
        "SELECT substr(timestamp, 1, 10), COUNT(*),
                SUM(input_tokens_est), SUM(output_tokens_est),
                SUM(input_tokens_est - output_tokens_est),
                COALESCE(SUM(pipe_override),0)
         FROM events
         GROUP BY substr(timestamp, 1, 10)
         ORDER BY substr(timestamp, 1, 10) DESC",
    )?;

    let rows = stmt.query_map([], map_aggregate_row)?;

    let mut result = Vec::new();
    for row in rows {
        let (date, commands, input_tokens, output_tokens, tokens_saved, pipe_override_count) =
            row.context("read daily row")?;
        result.push(DailyGain {
            date,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct: savings_pct(input_tokens, tokens_saved),
            pipe_override_count,
        });
    }
    Ok(result)
}

/// Returns the last successfully synced event ID (from `sync_state` table, default 0).
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn get_last_synced_id(conn: &Connection) -> anyhow::Result<i64> {
    let id: Option<String> = conn
        .query_row(
            "SELECT value FROM sync_state WHERE key = 'last_synced_id'",
            [],
            |r| r.get(0),
        )
        .optional()
        .context("query last_synced_id")?;
    Ok(id.and_then(|s| s.parse().ok()).unwrap_or(0))
}

/// Persist the last successfully synced event ID.
///
/// # Errors
/// Returns an error if the SQL INSERT/UPDATE fails.
pub fn set_last_synced_id(conn: &Connection, id: i64) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES ('last_synced_id', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![id.to_string()],
    )
    .context("set last_synced_id")?;
    Ok(())
}

/// Returns the timestamp of the last successful sync (from `sync_state` table).
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn get_last_synced_at(conn: &Connection) -> anyhow::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM sync_state WHERE key = 'last_synced_at'",
        [],
        |r| r.get(0),
    )
    .optional()
    .context("query last_synced_at")
}

/// Persist the timestamp of the last successful sync.
///
/// # Errors
/// Returns an error if the SQL INSERT/UPDATE fails.
pub fn set_last_synced_at(conn: &Connection, timestamp: &str) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES ('last_synced_at', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![timestamp],
    )
    .context("set last_synced_at")?;
    Ok(())
}

/// Returns the count of events that have not yet been synced.
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn get_pending_count(conn: &Connection) -> anyhow::Result<i64> {
    let last_id = get_last_synced_id(conn)?;
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE id > ?1",
        rusqlite::params![last_id],
        |r| r.get(0),
    )
    .context("query pending count")
}

/// An event ready to be shipped to the remote server.
pub struct SyncableEvent {
    pub id: i64,
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub input_tokens_est: i64,
    pub output_tokens_est: i64,
    pub timestamp: String,
}

/// Backfill `filter_hash` for existing events that have a `filter_name` but no hash.
///
/// For each distinct `filter_name` in the DB where `filter_hash IS NULL`, looks up the
/// current hash from the provided filter list and updates all matching rows.
///
/// Returns `(updated_rows, not_found_names)` where `not_found_names` lists filter names
/// that no longer resolve to any discovered filter (removed or renamed).
///
/// # Errors
/// Returns an error if the DB query or update fails.
pub fn backfill_filter_hashes(
    conn: &Connection,
    filters: &[crate::config::ResolvedFilter],
) -> anyhow::Result<(usize, Vec<String>)> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT filter_name FROM events \
         WHERE filter_hash IS NULL AND filter_name IS NOT NULL",
    )?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .filter_map(std::result::Result::ok)
        .collect();

    let mut updated = 0usize;
    let mut not_found = Vec::new();

    for name in &names {
        if let Some(rf) = filters.iter().find(|f| f.matches_name(name)) {
            let rows = conn.execute(
                "UPDATE events SET filter_hash = ?1 \
                 WHERE filter_name = ?2 AND filter_hash IS NULL",
                rusqlite::params![rf.hash, name],
            )?;
            updated += rows;
        } else {
            not_found.push(name.clone());
        }
    }

    Ok((updated, not_found))
}

/// Returns up to 500 events with `id > last_id`, ordered ascending.
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn get_events_since(conn: &Connection, last_id: i64) -> anyhow::Result<Vec<SyncableEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, filter_name, filter_hash, input_tokens_est, output_tokens_est, timestamp
         FROM events WHERE id > ?1 ORDER BY id ASC LIMIT 500",
    )?;
    let rows = stmt.query_map(rusqlite::params![last_id], |row| {
        Ok(SyncableEvent {
            id: row.get(0)?,
            filter_name: row.get(1)?,
            filter_hash: row.get(2)?,
            input_tokens_est: row.get(3)?,
            output_tokens_est: row.get(4)?,
            timestamp: row.get(5)?,
        })
    })?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.context("read sync event")?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_backfill;

#[cfg(test)]
mod tests_sync_state;
