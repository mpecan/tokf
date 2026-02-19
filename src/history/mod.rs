use std::path::PathBuf;

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

/// Returns the history DB path, following the same pattern as tracking DB.
/// Uses the same DB file as tracking to keep everything in one place.
pub fn db_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TOKF_DB_PATH") {
        return Some(PathBuf::from(p));
    }
    dirs::data_local_dir().map(|d| d.join("tokf").join("tracking.db"))
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
#[allow(clippy::too_many_arguments)]
pub fn record_history(
    conn: &Connection,
    command: &str,
    filter_name: Option<&str>,
    raw_output: &str,
    filtered_output: &str,
    exit_code: i32,
    config: &HistoryConfig,
) -> anyhow::Result<()> {
    // Insert new entry
    conn.execute(
        "INSERT INTO history
            (timestamp, command, filter_name, raw_output, filtered_output, exit_code)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![command, filter_name, raw_output, filtered_output, exit_code],
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

    let rows = stmt.query_map([limit_i64], |row| {
        Ok(HistoryEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            command: row.get(2)?,
            filter_name: row.get(3)?,
            raw_output: row.get(4)?,
            filtered_output: row.get(5)?,
            exit_code: row.get(6)?,
        })
    })?;

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
        Ok(Some(HistoryEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            command: row.get(2)?,
            filter_name: row.get(3)?,
            raw_output: row.get(4)?,
            filtered_output: row.get(5)?,
            exit_code: row.get(6)?,
        }))
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

    let rows = stmt.query_map(rusqlite::params![search_pattern, limit_i64], |row| {
        Ok(HistoryEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            command: row.get(2)?,
            filter_name: row.get(3)?,
            raw_output: row.get(4)?,
            filtered_output: row.get(5)?,
            exit_code: row.get(6)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.context("read history row")?);
    }
    Ok(result)
}

/// Clear all history entries.
///
/// # Errors
/// Returns an error if the DELETE operation fails.
pub fn clear_history(conn: &Connection) -> anyhow::Result<()> {
    conn.execute("DELETE FROM history", [])
        .context("clear history")?;
    Ok(())
}

#[cfg(test)]
mod tests;
