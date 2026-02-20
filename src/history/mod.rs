use anyhow::Context as _;
use rusqlite::Connection;

mod config;
mod queries;
mod types;

pub use config::{HistoryConfig, current_project, project_root_for};
pub use queries::{clear_history, get_history_entry, list_history, record_history, search_history};
pub use types::{HistoryEntry, HistoryRecord};

/// Open the shared tracking database and ensure the history schema is initialized.
///
/// # Errors
/// Returns an error if the DB cannot be opened or the schema cannot be created.
pub fn open_db(path: &std::path::Path) -> anyhow::Result<Connection> {
    let conn = crate::tracking::open_db(path)?;
    init_history_table(&conn)?;
    Ok(conn)
}

/// Initialize the history table and migrate existing DBs that lack the `project` column.
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
    let has_project: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name='project'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_project == 0 {
        conn.execute_batch("ALTER TABLE history ADD COLUMN project TEXT NOT NULL DEFAULT '';")
            .context("migrate history table: add project column")?;
    }

    // Create the project index after ensuring the column exists (fresh or migrated).
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_history_project ON history(project);")
        .context("create project index")?;

    Ok(())
}

/// Record a filtered command run to history, swallowing errors unless `TOKF_DEBUG` is set.
///
/// Only records commands where a filter was applied. Passthrough runs (no filter)
/// are excluded because raw and filtered output would be identical.
pub fn try_record(
    command: &str,
    filter_name: &str,
    raw_output: &str,
    filtered_output: &str,
    exit_code: i32,
) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = project_root_for(&cwd);
    let project = project_root.to_string_lossy().into_owned();
    let config = HistoryConfig::load(Some(&project_root));

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
    let record = HistoryRecord {
        project,
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
mod config_tests;
#[cfg(test)]
mod tests;
