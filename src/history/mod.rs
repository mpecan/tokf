use anyhow::Context as _;
use rusqlite::Connection;

/// A single history entry recording both raw and filtered output
#[derive(Debug)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String,
    pub command: String,
    pub filter_name: Option<String>,
    pub raw_output: String,
    pub filtered_output: String,
    pub exit_code: i32,
}

/// Parameters for recording one history entry.
pub struct HistoryRecord {
    pub command: String,
    pub filter_name: Option<String>,
    pub raw_output: String,
    pub filtered_output: String,
    pub exit_code: i32,
}

/// Configuration for history retention
#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub retention_count: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            retention_count: 10,
        }
    }
}

impl HistoryConfig {
    /// Load configuration from environment variables.
    /// `TOKF_HISTORY_RETENTION` sets the number of entries to keep (default: 10).
    pub fn from_env() -> Self {
        let retention_count = std::env::var("TOKF_HISTORY_RETENTION")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        Self { retention_count }
    }
}

/// Open the shared tracking database and ensure the history schema is initialized.
///
/// Calls [`crate::tracking::open_db`] for the events table, then initializes
/// the history table on the same connection.
///
/// # Errors
/// Returns an error if the DB cannot be opened or the schema cannot be created.
pub fn open_db(path: &std::path::Path) -> anyhow::Result<Connection> {
    let conn = crate::tracking::open_db(path)?;
    init_history_table(&conn)?;
    Ok(conn)
}

/// Initialize the history table in the existing database.
/// This should be called when opening the DB to ensure the schema exists.
///
/// # Errors
/// Returns an error if the table creation fails.
pub fn init_history_table(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT    NOT NULL,
            command           TEXT    NOT NULL,
            filter_name       TEXT,
            raw_output        TEXT    NOT NULL,
            filtered_output   TEXT    NOT NULL,
            exit_code         INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_history_command ON history(command);",
    )
    .context("create history table")?;
    Ok(())
}

/// Record a history entry and enforce retention policy.
///
/// # Errors
/// Returns an error if the INSERT or DELETE operations fail.
pub fn record_history(
    conn: &Connection,
    record: &HistoryRecord,
    config: &HistoryConfig,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO history
            (timestamp, command, filter_name, raw_output, filtered_output, exit_code)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            record.command,
            record.filter_name,
            record.raw_output,
            record.filtered_output,
            record.exit_code
        ],
    )
    .context("insert history entry")?;

    // Enforce retention limit by deleting oldest entries
    let retention_i64 = i64::from(config.retention_count);
    conn.execute(
        "DELETE FROM history
         WHERE id NOT IN (
             SELECT id FROM history
             ORDER BY id DESC
             LIMIT ?1
         )",
        rusqlite::params![retention_i64],
    )
    .context("enforce history retention")?;

    Ok(())
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        command: row.get(2)?,
        filter_name: row.get(3)?,
        raw_output: row.get(4)?,
        filtered_output: row.get(5)?,
        exit_code: row.get(6)?,
    })
}

/// List recent history entries, limited by count.
///
/// # Errors
/// Returns an error if the query fails.
pub fn list_history(conn: &Connection, limit: usize) -> anyhow::Result<Vec<HistoryEntry>> {
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, command, filter_name, raw_output, filtered_output, exit_code
         FROM history
         ORDER BY id DESC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map([limit_i64], map_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.context("read history row")?);
    }
    Ok(result)
}

/// Get a specific history entry by ID.
///
/// # Errors
/// Returns an error if the query fails or entry not found.
pub fn get_history_entry(conn: &Connection, id: i64) -> anyhow::Result<Option<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, command, filter_name, raw_output, filtered_output, exit_code
         FROM history
         WHERE id = ?1",
    )?;

    let mut rows = stmt.query([id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(map_row(row)?))
    } else {
        Ok(None)
    }
}

/// Search history entries by command or output content.
///
/// # Errors
/// Returns an error if the query fails.
pub fn search_history(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<HistoryEntry>> {
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;
    let search_pattern = format!("%{query}%");

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, command, filter_name, raw_output, filtered_output, exit_code
         FROM history
         WHERE command LIKE ?1
            OR raw_output LIKE ?1
            OR filtered_output LIKE ?1
         ORDER BY id DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![search_pattern, limit_i64], map_row)?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.context("read history row")?);
    }
    Ok(result)
}

/// Clear all history entries and reset the AUTOINCREMENT ID sequence.
///
/// # Errors
/// Returns an error if the DELETE operation fails.
pub fn clear_history(conn: &Connection) -> anyhow::Result<()> {
    conn.execute("DELETE FROM history", [])
        .context("clear history")?;
    // Reset the AUTOINCREMENT counter so new entries restart from id=1 after a clear.
    // Best-effort: sqlite_sequence is only populated after the first INSERT, so if
    // history was never written there is no row to reset — we ignore that case.
    let _ = conn.execute("DELETE FROM sqlite_sequence WHERE name='history'", []);
    Ok(())
}

/// Record a filtered command run to history, swallowing errors unless `TOKF_DEBUG` is set.
///
/// Only records commands where a filter was applied. Passthrough runs (no filter)
/// are excluded because raw and filtered output would be identical — storing them
/// wastes space and adds noise to history.
pub fn try_record(
    command: &str,
    filter_name: &str,
    raw_output: &str,
    filtered_output: &str,
    exit_code: i32,
) {
    let Some(path) = crate::tracking::db_path() else {
        return;
    };
    let conn = match open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            if std::env::var("TOKF_DEBUG").is_ok() {
                eprintln!("[tokf] history error (db open): {e:#}");
            }
            return;
        }
    };
    let config = HistoryConfig::from_env();
    let record = HistoryRecord {
        command: command.to_owned(),
        filter_name: Some(filter_name.to_owned()),
        raw_output: raw_output.to_owned(),
        filtered_output: filtered_output.to_owned(),
        exit_code,
    };
    if let Err(e) = record_history(&conn, &record, &config)
        && std::env::var("TOKF_DEBUG").is_ok()
    {
        eprintln!("[tokf] history error (record): {e:#}");
    }
}

#[cfg(test)]
mod tests;
