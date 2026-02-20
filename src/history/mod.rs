use anyhow::Context as _;
use rusqlite::Connection;

/// A single history entry recording both raw and filtered output
#[derive(Debug)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String,
    pub project: String,
    pub command: String,
    pub filter_name: Option<String>,
    pub raw_output: String,
    pub filtered_output: String,
    pub exit_code: i32,
}

/// Parameters for recording one history entry.
pub struct HistoryRecord {
    pub project: String,
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

/// Private: parsed representation of `.tokf/config.toml`.
#[derive(serde::Deserialize, Default)]
struct TokfProjectConfig {
    history: Option<TokfHistorySection>,
}

#[derive(serde::Deserialize)]
struct TokfHistorySection {
    retention: Option<u32>,
}

impl HistoryConfig {
    /// Load retention config. Priority:
    /// 1. `{project_root}/.tokf/config.toml` `[history] retention`
    /// 2. `TOKF_HISTORY_RETENTION` env var
    /// 3. Default: 10
    pub fn load(project_root: Option<&std::path::Path>) -> Self {
        let from_file = project_root.and_then(|root| {
            let path = root.join(".tokf").join("config.toml");
            let content = std::fs::read_to_string(&path).ok()?;
            let cfg: TokfProjectConfig = toml::from_str(&content).ok()?;
            cfg.history?.retention
        });

        let retention_count = from_file
            .or_else(|| std::env::var("TOKF_HISTORY_RETENTION").ok()?.parse().ok())
            .unwrap_or(10);

        Self { retention_count }
    }
}

/// Walk up from `dir` to find the nearest ancestor containing `.git` or `.tokf/`.
/// Falls back to `dir` itself if neither is found.
pub fn project_root_for(dir: &std::path::Path) -> std::path::PathBuf {
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() || current.join(".tokf").is_dir() {
            return current;
        }
        if !current.pop() {
            break;
        }
    }
    dir.to_path_buf()
}

/// Returns the current project root as a string (stored in the `project` column).
pub fn current_project() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    project_root_for(&cwd).to_string_lossy().into_owned()
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

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
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
mod tests;
