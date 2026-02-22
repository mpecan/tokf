use std::path::{Path, PathBuf};

use anyhow::Context as _;
use rusqlite::Connection;

pub use tokf_common::tracking::types::{DailyGain, FilterGain, GainSummary, TrackingEvent};

/// Returns the DB path: `TOKF_DB_PATH` env var overrides; else
/// `dirs::data_local_dir()/tokf/tracking.db`.
pub fn db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TOKF_DB_PATH") {
        return Some(PathBuf::from(p));
    }
    dirs::data_local_dir().map(|d| d.join("tokf").join("tracking.db"))
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
    let conn = Connection::open(path).with_context(|| format!("open db at {}", path.display()))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT    NOT NULL,
            command           TEXT    NOT NULL,
            filter_name       TEXT,
            input_bytes       INTEGER NOT NULL,
            output_bytes      INTEGER NOT NULL,
            input_tokens_est  INTEGER NOT NULL,
            output_tokens_est INTEGER NOT NULL,
            filter_time_ms    INTEGER NOT NULL,
            exit_code         INTEGER NOT NULL
        );",
    )
    .context("create events table")?;
    Ok(conn)
}

/// Pure constructor — no I/O. Computes token estimates from bytes.
#[allow(clippy::too_many_arguments)]
pub fn build_event(
    command: &str,
    filter_name: Option<&str>,
    input_bytes: usize,
    output_bytes: usize,
    filter_time_ms: u128,
    exit_code: i32,
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
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        input_bytes: input_bytes as i64,
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        output_bytes: output_bytes as i64,
        input_tokens_est,
        output_tokens_est,
        filter_time_ms: filter_time_ms_i64,
        exit_code,
    }
}

/// Insert one row; timestamp set by `SQLite` `strftime` in the SQL.
///
/// # Errors
/// Returns an error if the INSERT fails.
pub fn record_event(conn: &Connection, event: &TrackingEvent) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO events
            (timestamp, command, filter_name,
             input_bytes, output_bytes,
             input_tokens_est, output_tokens_est,
             filter_time_ms, exit_code)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            event.command,
            event.filter_name,
            event.input_bytes,
            event.output_bytes,
            event.input_tokens_est,
            event.output_tokens_est,
            event.filter_time_ms,
            event.exit_code,
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
                    COALESCE(SUM(input_tokens_est - output_tokens_est),0)
             FROM events",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .context("query summary")?;

    let (total_commands, total_input_tokens, total_output_tokens, tokens_saved) = row;
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
    })
}

/// # Errors
/// Returns an error if the SQL query fails.
pub fn query_by_filter(conn: &Connection) -> anyhow::Result<Vec<FilterGain>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(filter_name, 'passthrough'), COUNT(*),
                SUM(input_tokens_est), SUM(output_tokens_est),
                SUM(input_tokens_est - output_tokens_est)
         FROM events
         GROUP BY filter_name
         ORDER BY SUM(input_tokens_est - output_tokens_est) DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let input_tokens: i64 = row.get(2)?;
        let tokens_saved: i64 = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            input_tokens,
            row.get::<_, i64>(3)?,
            tokens_saved,
        ))
    })?;

    let mut result = Vec::new();
    for row in rows {
        let (filter_name, commands, input_tokens, output_tokens, tokens_saved) =
            row.context("read filter row")?;
        #[allow(clippy::cast_precision_loss)]
        let savings_pct = if input_tokens == 0 {
            0.0
        } else {
            tokens_saved as f64 / input_tokens as f64 * 100.0
        };
        result.push(FilterGain {
            filter_name,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct,
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
                SUM(input_tokens_est - output_tokens_est)
         FROM events
         GROUP BY substr(timestamp, 1, 10)
         ORDER BY substr(timestamp, 1, 10) DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let input_tokens: i64 = row.get(2)?;
        let tokens_saved: i64 = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            input_tokens,
            row.get::<_, i64>(3)?,
            tokens_saved,
        ))
    })?;

    let mut result = Vec::new();
    for row in rows {
        let (date, commands, input_tokens, output_tokens, tokens_saved) =
            row.context("read daily row")?;
        #[allow(clippy::cast_precision_loss)]
        let savings_pct = if input_tokens == 0 {
            0.0
        } else {
            tokens_saved as f64 / input_tokens as f64 * 100.0
        };
        result.push(DailyGain {
            date,
            commands,
            input_tokens,
            output_tokens,
            tokens_saved,
            savings_pct,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
