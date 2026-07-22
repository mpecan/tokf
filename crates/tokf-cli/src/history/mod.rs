use anyhow::Context as _;
use rusqlite::Connection;

use crate::runtime::Runtime;

mod config;
mod queries;
mod types;

pub use config::{
    HistoryConfig, OutputConfig, ShimsConfig, SyncConfig, TokfHistorySection, TokfOutputSection,
    TokfProjectConfig, TokfShimsSection, TokfSyncSection, current_project, global_config_path,
    load_project_config, local_config_path, project_root_for, save_project_config,
    save_upload_stats, save_upload_stats_to_path,
};
pub use queries::{
    clear_history, get_history_entry, get_latest_entry, list_history, record_history,
    search_history,
};
pub use types::{HistoryEntry, HistoryRecord};

/// Return `true` when `command` matches the most recent history entry for the
/// current project.  Errors are silently ignored (returns `false`).
///
/// This is used to detect when a caller re-runs the same command without
/// acting on previous filtered output — a signal that they may need the
/// full, unfiltered content.
pub fn try_was_recently_run(rt: &Runtime, command: &str) -> bool {
    let project = current_project(rt);

    let Some(path) = rt.tracking_db_path() else {
        return false;
    };
    let Ok(conn) = open_db(&path) else {
        return false;
    };
    matches!(
        queries::most_recent_command(&conn, &project),
        Ok(Some(last)) if last == command
    )
}

/// Open the shared tracking database and ensure the history schema is initialized.
///
/// # Errors
/// Returns an error if the DB cannot be opened or the schema cannot be created.
pub fn open_db(path: &std::path::Path) -> anyhow::Result<Connection> {
    let conn = crate::tracking::open_db(path)?;
    init_history_table(&conn)?;
    Ok(conn)
}

/// Return `true` when the `history` table already has a column named `column`.
fn has_column(conn: &Connection, column: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = ?1",
            [column],
            |r| r.get(0),
        )
        .unwrap_or(0);
    count > 0
}

/// Initialize the history table and migrate existing DBs that lack the
/// `project` or `executed_command` columns.
///
/// # Errors
/// Returns an error if the table creation or migration fails.
pub fn init_history_table(conn: &Connection) -> anyhow::Result<()> {
    // Create the table with the full current schema (no-op if it already exists).
    // Do NOT create idx_history_project here: if the table already exists without
    // the project column the index creation would fail. We handle that below.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT    NOT NULL,
            project           TEXT    NOT NULL DEFAULT '',
            command           TEXT    NOT NULL,
            executed_command  TEXT,
            filter_name       TEXT,
            raw_output        TEXT    NOT NULL,
            filtered_output   TEXT    NOT NULL,
            exit_code         INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_history_command ON history(command);",
    )
    .context("create history table")?;

    // Migration: add project column when upgrading from a schema without it.
    if !has_column(conn, "project") {
        conn.execute_batch("ALTER TABLE history ADD COLUMN project TEXT NOT NULL DEFAULT '';")
            .context("migrate history table: add project column")?;
    }

    // Migration: add executed_command when upgrading from a schema without it.
    // Nullable with no default — pre-migration rows genuinely don't know what
    // was executed, and `NULL` says that rather than claiming it was `command`.
    if !has_column(conn, "executed_command") {
        conn.execute_batch("ALTER TABLE history ADD COLUMN executed_command TEXT;")
            .context("migrate history table: add executed_command column")?;
    }

    // Create the project index after ensuring the column exists (fresh or migrated).
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_history_project ON history(project);")
        .context("create project index")?;

    Ok(())
}

/// A completed filter run, ready to be recorded to history.
pub struct RecordedRun<'a> {
    pub command: &'a str,
    /// The command actually executed when the filter's `run` override replaced
    /// `command`. `None` when `command` was run verbatim.
    pub executed_command: Option<&'a str>,
    pub filter_name: &'a str,
    pub raw_output: &'a str,
    pub filtered_output: &'a str,
    pub exit_code: i32,
}

/// Record a filtered command run to history, swallowing errors unless `TOKF_DEBUG` is set.
///
/// Only records commands where a filter was applied. Passthrough runs (no filter)
/// are excluded because raw and filtered output would be identical.
///
/// Returns `Some(id)` with the new history entry ID on success, `None` on error.
pub fn try_record(rt: &Runtime, run: &RecordedRun<'_>) -> Option<i64> {
    let RecordedRun {
        command,
        executed_command,
        filter_name,
        raw_output,
        filtered_output,
        exit_code,
    } = *run;
    let project_root = project_root_for(rt.cwd().unwrap_or_else(|| std::path::Path::new("")));
    let project = project_root.to_string_lossy().into_owned();
    let config = HistoryConfig::load(rt, Some(&project_root));

    let path = rt.tracking_db_path()?;
    let conn = match open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            if rt.debug() {
                eprintln!("[tokf] history error (db open): {e:#}");
            }
            return None;
        }
    };
    let record = HistoryRecord {
        project,
        command: command.to_owned(),
        executed_command: executed_command.map(ToOwned::to_owned),
        filter_name: Some(filter_name.to_owned()),
        raw_output: raw_output.to_owned(),
        filtered_output: filtered_output.to_owned(),
        exit_code,
    };
    match record_history(&conn, &record, &config) {
        Ok(id) => Some(id),
        Err(e) => {
            if rt.debug() {
                eprintln!("[tokf] history error (record): {e:#}");
            }
            None
        }
    }
}

#[cfg(test)]
mod config_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_clear;
#[cfg(test)]
mod tests_search;
