use serde::{Deserialize, Serialize};

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

/// Parsed representation of a tokf config file.
#[derive(Serialize, Deserialize, Default)]
pub struct TokfProjectConfig {
    pub history: Option<TokfHistorySection>,
    pub sync: Option<TokfSyncSection>,
}

#[derive(Serialize, Deserialize)]
pub struct TokfHistorySection {
    pub retention: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct TokfSyncSection {
    pub auto_sync_threshold: Option<u32>,
    pub upload_usage_stats: Option<bool>,
}

/// Read `[history] retention` from a TOML config file path. Returns `None` on any error.
fn read_retention_from_config(path: &std::path::Path) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    let cfg: TokfProjectConfig = toml::from_str(&content).ok()?;
    cfg.history?.retention
}

/// Read `[sync] auto_sync_threshold` from a TOML config file path. Returns `None` on any error.
fn read_sync_threshold_from_config(path: &std::path::Path) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    let cfg: TokfProjectConfig = toml::from_str(&content).ok()?;
    cfg.sync?.auto_sync_threshold
}

/// Read `[sync] upload_usage_stats` from a TOML config file path. Returns `None` on any error.
fn read_upload_usage_stats_from_config(path: &std::path::Path) -> Option<bool> {
    let content = std::fs::read_to_string(path).ok()?;
    let cfg: TokfProjectConfig = toml::from_str(&content).ok()?;
    cfg.sync?.upload_usage_stats
}

impl HistoryConfig {
    /// Load retention config using auto-detected paths. Priority:
    /// 1. `{project_root}/.tokf/config.toml` `[history] retention`
    /// 2. `{config_dir}/tokf/config.toml` `[history] retention`  (e.g. `~/.config/tokf/config.toml`)
    /// 3. Default: 10
    pub fn load(project_root: Option<&std::path::Path>) -> Self {
        let global = crate::paths::user_dir().map(|d| d.join("config.toml"));
        Self::load_from(project_root, global.as_deref())
    }

    /// Load retention config from explicit paths. Useful for testing.
    /// Priority: project config → global config → default 10.
    pub fn load_from(
        project_root: Option<&std::path::Path>,
        global_config: Option<&std::path::Path>,
    ) -> Self {
        let from_project = project_root
            .and_then(|root| read_retention_from_config(&root.join(".tokf").join("config.toml")));
        let from_global = global_config.and_then(read_retention_from_config);
        let retention_count = from_project.or(from_global).unwrap_or(10);
        Self { retention_count }
    }
}

/// Configuration for auto-sync behavior
#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub auto_sync_threshold: u32,
    pub upload_usage_stats: Option<bool>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            auto_sync_threshold: 100,
            upload_usage_stats: None,
        }
    }
}

impl SyncConfig {
    /// Load sync config using auto-detected paths. Priority:
    /// 1. `{project_root}/.tokf/config.toml` `[sync] auto_sync_threshold`
    /// 2. `{config_dir}/tokf/config.toml` `[sync] auto_sync_threshold`
    /// 3. Default: 100
    pub fn load(project_root: Option<&std::path::Path>) -> Self {
        let global = crate::paths::user_dir().map(|d| d.join("config.toml"));
        Self::load_from(project_root, global.as_deref())
    }

    /// Load sync config from explicit paths. Useful for testing.
    /// Priority: project config → global config → default 100.
    pub fn load_from(
        project_root: Option<&std::path::Path>,
        global_config: Option<&std::path::Path>,
    ) -> Self {
        let from_project = project_root.and_then(|root| {
            read_sync_threshold_from_config(&root.join(".tokf").join("config.toml"))
        });
        let from_global = global_config.and_then(read_sync_threshold_from_config);
        let auto_sync_threshold = from_project.or(from_global).unwrap_or(100);

        let upload_from_project = project_root.and_then(|root| {
            read_upload_usage_stats_from_config(&root.join(".tokf").join("config.toml"))
        });
        let upload_from_global = global_config.and_then(read_upload_usage_stats_from_config);
        let upload_usage_stats = upload_from_project.or(upload_from_global);

        Self {
            auto_sync_threshold,
            upload_usage_stats,
        }
    }
}

/// Load a `TokfProjectConfig` from a TOML file path. Returns `Default` on any error.
pub fn load_project_config(path: &std::path::Path) -> TokfProjectConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| toml::from_str(&c).ok())
        .unwrap_or_default()
}

/// Save a `TokfProjectConfig` to a TOML file path.
///
/// Creates parent directories if needed. Uses restrictive permissions on Unix.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be written.
pub fn save_project_config(
    path: &std::path::Path,
    config: &TokfProjectConfig,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    crate::fs::write_config_file(path, &content)
}

/// Persist the `upload_usage_stats` preference to the global `config.toml`.
///
/// Loads the existing config, sets the field, and writes back.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined or the file
/// cannot be written.
pub fn save_upload_stats(enabled: bool) -> anyhow::Result<()> {
    let path = crate::paths::user_dir()
        .map(|d| d.join("config.toml"))
        .ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    save_upload_stats_to_path(&path, enabled)
}

/// Core logic for persisting `upload_usage_stats` to a specific config file path.
///
/// Separated from [`save_upload_stats`] to allow direct testing without
/// depending on the platform config directory.
///
/// # Errors
///
/// Returns an error if the config file cannot be written.
pub fn save_upload_stats_to_path(path: &std::path::Path, enabled: bool) -> anyhow::Result<()> {
    let mut config = load_project_config(path);
    let sync = config.sync.get_or_insert(TokfSyncSection {
        auto_sync_threshold: None,
        upload_usage_stats: None,
    });
    sync.upload_usage_stats = Some(enabled);
    save_project_config(path, &config)
}

/// Returns the global config.toml path.
pub fn global_config_path() -> Option<std::path::PathBuf> {
    crate::paths::user_dir().map(|d| d.join("config.toml"))
}

/// Returns the local (project) config.toml path for a given project root.
pub fn local_config_path(project_root: &std::path::Path) -> std::path::PathBuf {
    project_root.join(".tokf").join("config.toml")
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
