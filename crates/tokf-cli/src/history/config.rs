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

/// Private: parsed representation of a tokf config file.
#[derive(serde::Deserialize, Default)]
struct TokfProjectConfig {
    history: Option<TokfHistorySection>,
}

#[derive(serde::Deserialize)]
struct TokfHistorySection {
    retention: Option<u32>,
}

/// Read `[history] retention` from a TOML config file path. Returns `None` on any error.
fn read_retention_from_config(path: &std::path::Path) -> Option<u32> {
    let content = std::fs::read_to_string(path).ok()?;
    let cfg: TokfProjectConfig = toml::from_str(&content).ok()?;
    cfg.history?.retention
}

impl HistoryConfig {
    /// Load retention config using auto-detected paths. Priority:
    /// 1. `{project_root}/.tokf/config.toml` `[history] retention`
    /// 2. `{config_dir}/tokf/config.toml` `[history] retention`  (e.g. `~/.config/tokf/config.toml`)
    /// 3. Default: 10
    pub fn load(project_root: Option<&std::path::Path>) -> Self {
        let global = dirs::config_dir().map(|d| d.join("tokf").join("config.toml"));
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
