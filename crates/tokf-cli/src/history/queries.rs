use anyhow::Context as _;
use rusqlite::Connection;

use super::config::HistoryConfig;
use super::types::{HistoryEntry, HistoryRecord};

pub(super) fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        timestamp: row.get(1)?,
        project: row.get(2)?,
        command: row.get(3)?,
        filter_name: row.get(4)?,
        raw_output: row.get(5)?,
        filtered_output: row.get(6)?,
        exit_code: row.get(7)?,
    })
}

/// Record a history entry and enforce per-project retention policy.
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
            (timestamp, project, command, filter_name, raw_output, filtered_output, exit_code)
         VALUES
            (strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            record.project,
            record.command,
            record.filter_name,
            record.raw_output,
            record.filtered_output,
            record.exit_code
        ],
    )
    .context("insert history entry")?;

    // Retention is scoped per project so each project keeps its own N entries.
    let retention_i64 = i64::from(config.retention_count);
    conn.execute(
        "DELETE FROM history
         WHERE project = ?1
           AND id NOT IN (
               SELECT id FROM history
               WHERE project = ?1
               ORDER BY id DESC
               LIMIT ?2
           )",
        rusqlite::params![record.project, retention_i64],
    )
    .context("enforce history retention")?;

    Ok(())
}

/// List recent history entries.
///
/// Pass `project = Some("path")` to filter to one project, or `None` for all projects.
///
/// # Errors
/// Returns an error if the query fails.
pub fn list_history(
    conn: &Connection,
    limit: usize,
    project: Option<&str>,
) -> anyhow::Result<Vec<HistoryEntry>> {
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, project, command, filter_name,
                raw_output, filtered_output, exit_code
         FROM history
         WHERE (?1 IS NULL OR project = ?1)
         ORDER BY id DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![project, limit_i64], map_row)?;
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
        "SELECT id, timestamp, project, command, filter_name,
                raw_output, filtered_output, exit_code
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
/// Pass `project = Some("path")` to scope to one project, or `None` for all.
///
/// # Errors
/// Returns an error if the query fails.
pub fn search_history(
    conn: &Connection,
    query: &str,
    limit: usize,
    project: Option<&str>,
) -> anyhow::Result<Vec<HistoryEntry>> {
    #[allow(clippy::cast_possible_wrap)]
    let limit_i64 = limit as i64;
    let search_pattern = format!("%{query}%");

    let mut stmt = conn.prepare(
        "SELECT id, timestamp, project, command, filter_name,
                raw_output, filtered_output, exit_code
         FROM history
         WHERE (?1 IS NULL OR project = ?1)
           AND (command LIKE ?2 OR raw_output LIKE ?2 OR filtered_output LIKE ?2)
         ORDER BY id DESC
         LIMIT ?3",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![project, search_pattern, limit_i64],
        map_row,
    )?;
    let mut result = Vec::new();
    for row in rows {
        result.push(row.context("read history row")?);
    }
    Ok(result)
}

/// Clear history entries. Pass `project = Some("path")` to clear one project only,
/// or `None` to clear everything and reset the AUTOINCREMENT sequence.
///
/// # Errors
/// Returns an error if the DELETE operation fails.
pub fn clear_history(conn: &Connection, project: Option<&str>) -> anyhow::Result<()> {
    conn.execute(
        "DELETE FROM history WHERE (?1 IS NULL OR project = ?1)",
        rusqlite::params![project],
    )
    .context("clear history")?;

    if project.is_none() {
        // Reset AUTOINCREMENT counter only when clearing all entries.
        let _ = conn.execute("DELETE FROM sqlite_sequence WHERE name='history'", []);
    }

    Ok(())
}
