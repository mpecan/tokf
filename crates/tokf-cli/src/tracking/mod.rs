use std::path::Path;

use anyhow::Context as _;
use rusqlite::{Connection, OptionalExtension as _};

use tokf_common::tokens::estimate_tokens_from_bytes;
pub use tokf_common::tracking::types::{DailyGain, FilterGain, GainSummary, TrackingEvent};

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
            pipe_override     INTEGER NOT NULL DEFAULT 0,
            raw_bytes         INTEGER NOT NULL DEFAULT 0,
            raw_tokens_est    INTEGER NOT NULL DEFAULT 0,
            project           TEXT    NOT NULL DEFAULT ''
        );",
    )
    .context("create events table")?;

    run_migrations(&conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_state (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )
    .context("create sync_state table")?;

    Ok(conn)
}

/// Add one or more columns when `probe_column` is absent from `events`.
///
/// Every migration below is the same shape — check `pragma_table_info`, then
/// `ALTER TABLE` — so the probe lives here rather than being spelled out once
/// per column.
fn add_columns_if_missing(
    conn: &Connection,
    probe_column: &str,
    ddl: &str,
    what: &str,
) -> anyhow::Result<()> {
    let present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name=?1",
            [probe_column],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if present == 0 {
        conn.execute_batch(ddl)
            .with_context(|| format!("migrate events table: add {what}"))?;
    }
    Ok(())
}

/// Run schema migrations for the events table.
fn run_migrations(conn: &Connection) -> anyhow::Result<()> {
    add_columns_if_missing(
        conn,
        "pipe_override",
        "ALTER TABLE events ADD COLUMN pipe_override INTEGER NOT NULL DEFAULT 0;",
        "pipe_override column",
    )?;

    add_columns_if_missing(
        conn,
        "filter_hash",
        "ALTER TABLE events ADD COLUMN filter_hash TEXT;",
        "filter_hash column",
    )?;

    add_columns_if_missing(
        conn,
        "raw_bytes",
        "ALTER TABLE events ADD COLUMN raw_bytes INTEGER NOT NULL DEFAULT 0;
         ALTER TABLE events ADD COLUMN raw_tokens_est INTEGER NOT NULL DEFAULT 0;
         UPDATE events SET raw_bytes = input_bytes, raw_tokens_est = input_tokens_est;",
        "raw_bytes columns",
    )?;

    // Pre-existing rows get the empty-string default — `tokf doctor` treats
    // empty as "unknown" and shows them under all projects.
    add_columns_if_missing(
        conn,
        "project",
        "ALTER TABLE events ADD COLUMN project TEXT NOT NULL DEFAULT '';",
        "project column",
    )?;

    // Pipeline capture. `head_exit_code != exit_code` is the swallowed-status
    // signal; `pipeline_tail` keeps capture rows out of the `passthrough`
    // bucket they would otherwise fall into.
    add_columns_if_missing(
        conn,
        "pipeline_tail",
        "ALTER TABLE events ADD COLUMN pipeline_tail TEXT;
         ALTER TABLE events ADD COLUMN head_exit_code INTEGER;",
        "pipeline capture columns",
    )?;

    // Indexes used by `tokf doctor` burst-detection and per-filter queries.
    // Created here (not in CREATE TABLE) so existing DBs pick them up too.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_events_command_timestamp \
            ON events(command, timestamp DESC);
         CREATE INDEX IF NOT EXISTS idx_events_filter_timestamp \
            ON events(filter_name, timestamp DESC);",
    )
    .context("create events indexes")?;

    Ok(())
}

/// Pure constructor — no I/O. Computes token estimates from bytes.
#[allow(clippy::too_many_arguments)]
pub fn build_event(
    command: &str,
    filter_name: Option<&str>,
    filter_hash: Option<&str>,
    input_bytes: usize,
    output_bytes: usize,
    raw_bytes: usize,
    filter_time_ms: u128,
    exit_code: i32,
    pipe_override: bool,
) -> TrackingEvent {
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let input_tokens_est = estimate_tokens_from_bytes(input_bytes) as i64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let output_tokens_est = estimate_tokens_from_bytes(output_bytes) as i64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let raw_tokens_est = estimate_tokens_from_bytes(raw_bytes) as i64;
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
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        raw_bytes: raw_bytes as i64,
        raw_tokens_est,
        filter_time_ms: filter_time_ms_i64,
        exit_code,
        pipe_override,
        // `project` defaults to empty here, and the pipeline-capture columns
        // to `None`. Callers that know better (`resolve::record_run` for the
        // project, `crate::pipeline` for capture) set them on the event before
        // passing it to `record_event`.
        pipeline_tail: None,
        head_exit_code: None,
        project: String::new(),
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
             raw_bytes, raw_tokens_est,
             filter_time_ms, exit_code, pipe_override, project,
             pipeline_tail, head_exit_code)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            event.command,
            event.filter_name,
            event.filter_hash,
            event.input_bytes,
            event.output_bytes,
            event.input_tokens_est,
            event.output_tokens_est,
            event.raw_bytes,
            event.raw_tokens_est,
            event.filter_time_ms,
            event.exit_code,
            i64::from(event.pipe_override),
            event.project,
            event.pipeline_tail,
            event.head_exit_code,
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
                    COALESCE(SUM(pipe_override),0),
                    COALESCE(SUM(filter_time_ms),0),
                    COALESCE(SUM(CASE WHEN raw_tokens_est = 0 THEN input_tokens_est ELSE raw_tokens_est END),0),
                    COALESCE(SUM(CASE WHEN head_exit_code IS NOT NULL
                                       AND head_exit_code != exit_code
                                      THEN 1 ELSE 0 END),0)
             FROM events",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
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
        total_filter_time_ms,
        total_raw_tokens,
        exit_mismatch_count,
    ) = row;
    Ok(GainSummary {
        total_commands,
        total_input_tokens,
        total_output_tokens,
        tokens_saved,
        savings_pct: percentage(tokens_saved, total_input_tokens),
        pipe_override_count,
        exit_mismatch_count,
        total_filter_time_ms,
        avg_filter_time_ms: mean(total_filter_time_ms, total_commands),
        total_raw_tokens,
    })
}

/// `part` as a percentage of `whole`, or `0.0` when `whole` is zero.
#[allow(clippy::cast_precision_loss)]
fn percentage(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64 * 100.0
    }
}

/// `total / count`, or `0.0` when `count` is zero.
#[allow(clippy::cast_precision_loss)]
fn mean(total: i64, count: i64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

/// Row type returned by aggregate queries.
type AggregateRow = (String, i64, i64, i64, i64, i64, i64, i64);

/// Shared row mapper for aggregate queries.
/// Returns `(label, commands, input, output, saved, pipe_overrides, filter_time_ms, raw_tokens)`.
fn map_aggregate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AggregateRow> {
    Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
        row.get::<_, i64>(3)?,
        row.get::<_, i64>(4)?,
        row.get::<_, i64>(5)?,
        row.get::<_, i64>(6)?,
        row.get::<_, i64>(7)?,
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
        // Capture rows are bucketed as `pipeline-capture`, not `passthrough`:
        // both have a NULL filter_name, but they mean different things. A
        // passthrough is a command tokf had no filter for; a capture is a
        // pipeline the caller reduced themselves, and lumping them together
        // would attribute the caller's own `| tail` to tokf's filter coverage.
        "SELECT CASE
                    WHEN filter_name IS NOT NULL THEN filter_name
                    WHEN pipeline_tail IS NOT NULL THEN 'pipeline-capture'
                    ELSE 'passthrough'
                END AS bucket,
                COUNT(*),
                SUM(input_tokens_est), SUM(output_tokens_est),
                SUM(input_tokens_est - output_tokens_est),
                COALESCE(SUM(pipe_override),0),
                COALESCE(SUM(filter_time_ms),0),
                COALESCE(SUM(CASE WHEN raw_tokens_est = 0 THEN input_tokens_est ELSE raw_tokens_est END),0)
         FROM events
         GROUP BY bucket
         ORDER BY SUM(input_tokens_est - output_tokens_est) DESC",
    )?;

    let rows = stmt.query_map([], map_aggregate_row)?;

    let mut result = Vec::new();
    for row in rows {
        let (
            filter_name,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            pipe_override_count,
            total_filter_time_ms,
            raw_tokens,
        ) = row.context("read filter row")?;
        #[allow(clippy::cast_precision_loss)]
        let avg_filter_time_ms = if commands == 0 {
            0.0
        } else {
            total_filter_time_ms as f64 / commands as f64
        };
        result.push(FilterGain {
            filter_name,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct: savings_pct(input_tokens, tokens_saved),
            pipe_override_count,
            total_filter_time_ms,
            avg_filter_time_ms,
            raw_tokens,
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
                COALESCE(SUM(pipe_override),0),
                COALESCE(SUM(filter_time_ms),0),
                COALESCE(SUM(CASE WHEN raw_tokens_est = 0 THEN input_tokens_est ELSE raw_tokens_est END),0)
         FROM events
         GROUP BY substr(timestamp, 1, 10)
         ORDER BY substr(timestamp, 1, 10) DESC",
    )?;

    let rows = stmt.query_map([], map_aggregate_row)?;

    let mut result = Vec::new();
    for row in rows {
        let (
            date,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            pipe_override_count,
            total_filter_time_ms,
            raw_tokens,
        ) = row.context("read daily row")?;
        result.push(DailyGain {
            date,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct: savings_pct(input_tokens, tokens_saved),
            pipe_override_count,
            total_filter_time_ms,
            raw_tokens,
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
    pub raw_tokens_est: i64,
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
        "SELECT id, filter_name, filter_hash, input_tokens_est, output_tokens_est,
                raw_tokens_est, timestamp
         FROM events WHERE id > ?1 ORDER BY id ASC LIMIT 500",
    )?;
    let rows = stmt.query_map(rusqlite::params![last_id], |row| {
        Ok(SyncableEvent {
            id: row.get(0)?,
            filter_name: row.get(1)?,
            filter_hash: row.get(2)?,
            input_tokens_est: row.get(3)?,
            output_tokens_est: row.get(4)?,
            raw_tokens_est: row.get(5)?,
            timestamp: row.get(6)?,
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
mod tests_pipe_override;

#[cfg(test)]
mod tests_project;

#[cfg(test)]
mod tests_raw_bytes;

#[cfg(test)]
mod tests_sync_state;
