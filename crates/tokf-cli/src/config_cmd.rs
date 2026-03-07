use clap::Subcommand;
use serde::Serialize;

use tokf::history::{
    self, HistoryConfig, ShimsConfig, SyncConfig, TokfProjectConfig, TokfShimsSection,
    TokfSyncSection, global_config_path, load_project_config, local_config_path, project_root_for,
    save_project_config,
};

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show all effective configuration with source paths
    Show {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print a single configuration value (for scripting)
    Get {
        /// Dotted key (`history.retention`, `shims.enabled`, `sync.auto_sync_threshold`, `sync.upload_stats`)
        key: String,
    },
    /// Set a configuration value
    Set {
        /// Dotted key (`history.retention`, `shims.enabled`, `sync.auto_sync_threshold`, `sync.upload_stats`)
        key: String,
        /// Value to set
        value: String,
        /// Write to project-local .tokf/config.toml instead of global config
        #[arg(long)]
        local: bool,
    },
    /// Print raw config file contents
    Print {
        /// Print global config file
        #[arg(long, conflicts_with = "local")]
        global: bool,
        /// Print local (project) config file
        #[arg(long, conflicts_with = "global")]
        local: bool,
    },
    /// Show config file paths with existence status
    Path,
}

pub fn run_config_action(action: &ConfigAction) -> i32 {
    match action {
        ConfigAction::Show { json } => cmd_config_show(*json),
        ConfigAction::Get { key } => cmd_config_get(key),
        ConfigAction::Set { key, value, local } => cmd_config_set(key, value, *local),
        ConfigAction::Print { global, local } => cmd_config_print(*global, *local),
        ConfigAction::Path => cmd_config_path(),
    }
}

// ── Supported keys ──────────────────────────────────────────────

const KNOWN_KEYS: &[&str] = &[
    "history.retention",
    "shims.enabled",
    "sync.auto_sync_threshold",
    "sync.upload_stats",
];

fn print_known_keys() {
    eprintln!("[tokf] known config keys:");
    for key in KNOWN_KEYS {
        eprintln!("  {key}");
    }
}

// ── config show ─────────────────────────────────────────────────

#[derive(Serialize)]
struct ConfigEntry {
    key: String,
    /// `None` when the value is unset (serialises as JSON `null`).
    value: Option<String>,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
}

fn cmd_config_show(json: bool) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = project_root_for(&cwd);
    let global_path = global_config_path();
    let local_path = local_config_path(&project_root);

    let entries = collect_config_entries(global_path.as_deref(), &local_path, &project_root);

    if json {
        crate::output::print_json(&entries);
    } else {
        println!("tokf configuration:");
        for entry in &entries {
            let source_display = match entry.source.as_str() {
                "default" => "(default)".to_string(),
                "local" => format!(
                    "(local: {})",
                    entry.file.as_deref().unwrap_or(".tokf/config.toml")
                ),
                "global" => format!(
                    "(global: {})",
                    entry.file.as_deref().unwrap_or("config.toml")
                ),
                other => format!("({other})"),
            };
            let display_value = entry.value.as_deref().unwrap_or("(not set)");
            println!("  {} = {display_value}  {source_display}", entry.key);
        }
    }
    0
}

fn collect_config_entries(
    global_path: Option<&std::path::Path>,
    local_path: &std::path::Path,
    project_root: &std::path::Path,
) -> Vec<ConfigEntry> {
    let history = HistoryConfig::load_from(Some(project_root), global_path);
    let shims = ShimsConfig::load_from(Some(project_root), global_path);
    let sync = SyncConfig::load_from(Some(project_root), global_path);

    let local_cfg = local_path
        .is_file()
        .then(|| load_project_config(local_path));
    let global_cfg = global_path.filter(|p| p.is_file()).map(load_project_config);

    let mut entries = Vec::new();

    let src = |has_field: fn(&TokfProjectConfig) -> bool| {
        find_source(
            local_cfg.as_ref(),
            local_path,
            global_cfg.as_ref(),
            global_path,
            has_field,
        )
    };

    // history.retention
    let (ret_source, ret_file) = src(|c| c.history.as_ref().and_then(|h| h.retention).is_some());
    entries.push(ConfigEntry {
        key: "history.retention".to_string(),
        value: Some(history.retention_count.to_string()),
        source: ret_source,
        file: ret_file,
    });

    // shims.enabled
    let (shims_source, shims_file) = src(|c| c.shims.as_ref().and_then(|s| s.enabled).is_some());
    entries.push(ConfigEntry {
        key: "shims.enabled".to_string(),
        value: Some(shims.enabled.to_string()),
        source: shims_source,
        file: shims_file,
    });

    // sync.auto_sync_threshold
    let (thresh_source, thresh_file) = src(|c| {
        c.sync
            .as_ref()
            .and_then(|s| s.auto_sync_threshold)
            .is_some()
    });
    entries.push(ConfigEntry {
        key: "sync.auto_sync_threshold".to_string(),
        value: Some(sync.auto_sync_threshold.to_string()),
        source: thresh_source,
        file: thresh_file,
    });

    // sync.upload_stats
    let (stats_source, stats_file) =
        src(|c| c.sync.as_ref().and_then(|s| s.upload_usage_stats).is_some());
    entries.push(ConfigEntry {
        key: "sync.upload_stats".to_string(),
        value: sync.upload_usage_stats.map(|b| b.to_string()),
        source: stats_source,
        file: stats_file,
    });

    entries
}

/// Determine which config source (local, global, or default) provides a given field.
///
/// `has_field` extracts the field from a `TokfProjectConfig`, returning `true` when present.
/// Checks local first, then global, falling back to `"default"`.
fn find_source(
    local_cfg: Option<&TokfProjectConfig>,
    local_path: &std::path::Path,
    global_cfg: Option<&TokfProjectConfig>,
    global_path: Option<&std::path::Path>,
    has_field: fn(&TokfProjectConfig) -> bool,
) -> (String, Option<String>) {
    if local_cfg.is_some_and(has_field) {
        return ("local".to_string(), Some(local_path.display().to_string()));
    }
    if global_cfg.is_some_and(has_field) {
        return (
            "global".to_string(),
            global_path.map(|p| p.display().to_string()),
        );
    }
    ("default".to_string(), None)
}

// ── config get ──────────────────────────────────────────────────

fn cmd_config_get(key: &str) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = project_root_for(&cwd);

    match key {
        "history.retention" => {
            let config = HistoryConfig::load(Some(&project_root));
            println!("{}", config.retention_count);
        }
        "shims.enabled" => {
            let config = ShimsConfig::load(Some(&project_root));
            println!("{}", config.enabled);
        }
        "sync.auto_sync_threshold" => {
            let config = SyncConfig::load(Some(&project_root));
            println!("{}", config.auto_sync_threshold);
        }
        "sync.upload_stats" => {
            let config = SyncConfig::load(Some(&project_root));
            if let Some(v) = config.upload_usage_stats {
                println!("{v}");
            } else {
                return 1;
            }
        }
        _ => {
            eprintln!("[tokf] unknown config key: {key}");
            print_known_keys();
            return 1;
        }
    }
    0
}

// ── config set ──────────────────────────────────────────────────

fn cmd_config_set(key: &str, value: &str, local: bool) -> i32 {
    let target_path = if local {
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_root = project_root_for(&cwd);
        local_config_path(&project_root)
    } else {
        let Some(p) = global_config_path() else {
            eprintln!("[tokf] cannot determine config directory");
            return 1;
        };
        p
    };

    match key {
        "history.retention" => set_parsed_field(
            &target_path,
            key,
            value,
            "a non-negative integer",
            |cfg, n| {
                cfg.history
                    .get_or_insert(history::TokfHistorySection { retention: None })
                    .retention = Some(n);
            },
        ),
        "shims.enabled" => {
            let rc = set_parsed_field(&target_path, key, value, "true or false", |cfg, b| {
                cfg.shims
                    .get_or_insert(TokfShimsSection { enabled: None })
                    .enabled = Some(b);
            });
            // Immediately remove stale shims when disabling
            if rc == 0
                && value == "false"
                && let Some(dir) = tokf::paths::shims_dir()
            {
                let _ = std::fs::remove_dir_all(dir);
            }
            rc
        }
        "sync.auto_sync_threshold" => set_parsed_field(
            &target_path,
            key,
            value,
            "a non-negative integer",
            |cfg, n| {
                cfg.sync
                    .get_or_insert(TokfSyncSection {
                        auto_sync_threshold: None,
                        upload_usage_stats: None,
                    })
                    .auto_sync_threshold = Some(n);
            },
        ),
        "sync.upload_stats" => set_upload_stats(&target_path, value),
        _ => {
            eprintln!("[tokf] unknown config key: {key}");
            print_known_keys();
            1
        }
    }
}

/// Parse a value of type `T` and apply it to the config via the given setter.
fn set_parsed_field<T: std::str::FromStr>(
    path: &std::path::Path,
    key: &str,
    value: &str,
    type_hint: &str,
    apply: fn(&mut TokfProjectConfig, T),
) -> i32 {
    let Ok(parsed) = value.parse::<T>() else {
        eprintln!("[tokf] invalid value for {key}: expected {type_hint}");
        return 1;
    };
    let mut config = load_project_config(path);
    apply(&mut config, parsed);
    if let Err(e) = save_project_config(path, &config) {
        eprintln!("[tokf] failed to write config: {e:#}");
        return 1;
    }
    0
}

fn set_upload_stats(path: &std::path::Path, value: &str) -> i32 {
    let Ok(b) = value.parse::<bool>() else {
        eprintln!("[tokf] invalid value for sync.upload_stats: expected true or false");
        return 1;
    };
    if let Err(e) = history::save_upload_stats_to_path(path, b) {
        eprintln!("[tokf] failed to write config: {e:#}");
        return 1;
    }
    0
}

// ── config print ────────────────────────────────────────────────

fn cmd_config_print(global: bool, local: bool) -> i32 {
    let path = if local {
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_root = project_root_for(&cwd);
        local_config_path(&project_root)
    } else if global {
        let Some(p) = global_config_path() else {
            eprintln!("[tokf] cannot determine global config directory");
            return 1;
        };
        p
    } else {
        // Default: try local first, fall back to global
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_root = project_root_for(&cwd);
        let local_path = local_config_path(&project_root);
        if local_path.is_file() {
            local_path
        } else {
            let Some(p) = global_config_path() else {
                eprintln!("[tokf] no config file found");
                return 1;
            };
            p
        }
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            print!("{content}");
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("[tokf] config file not found: {}", path.display());
            1
        }
        Err(e) => {
            eprintln!("[tokf] error reading {}: {e}", path.display());
            1
        }
    }
}

// ── config path ─────────────────────────────────────────────────

fn cmd_config_path() -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = project_root_for(&cwd);

    let global = global_config_path();
    let local = local_config_path(&project_root);

    print_path_line("global", global.as_deref());
    print_path_line("local", Some(&local));

    0
}

fn print_path_line(label: &str, path: Option<&std::path::Path>) {
    if let Some(p) = path {
        let status = if p.exists() { "exists" } else { "not found" };
        println!("{label:7} {} ({status})", p.display());
    } else {
        println!("{label:7} (unavailable)");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn collect_config_entries_defaults() {
        let dir = TempDir::new().unwrap();
        let local = dir.path().join(".tokf/config.toml");
        let entries = collect_config_entries(None, &local, dir.path());
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].key, "history.retention");
        assert_eq!(entries[0].value.as_deref(), Some("10"));
        assert_eq!(entries[0].source, "default");
    }

    #[test]
    fn collect_config_entries_from_local() {
        let dir = TempDir::new().unwrap();
        let tokf_dir = dir.path().join(".tokf");
        std::fs::create_dir_all(&tokf_dir).unwrap();
        let local = tokf_dir.join("config.toml");
        std::fs::write(&local, "[history]\nretention = 42\n").unwrap();

        let entries = collect_config_entries(None, &local, dir.path());
        assert_eq!(entries[0].value.as_deref(), Some("42"));
        assert_eq!(entries[0].source, "local");
    }

    #[test]
    fn collect_config_entries_from_global() {
        let dir = TempDir::new().unwrap();
        let global = dir.path().join("global_config.toml");
        std::fs::write(&global, "[sync]\nauto_sync_threshold = 200\n").unwrap();
        let local = dir.path().join("nonexistent/.tokf/config.toml");

        let entries = collect_config_entries(Some(&global), &local, dir.path());
        assert_eq!(entries[2].value.as_deref(), Some("200"));
        assert_eq!(entries[2].source, "global");
    }

    #[test]
    fn known_keys_are_valid() {
        assert!(KNOWN_KEYS.contains(&"history.retention"));
        assert!(KNOWN_KEYS.contains(&"shims.enabled"));
        assert!(KNOWN_KEYS.contains(&"sync.auto_sync_threshold"));
        assert!(KNOWN_KEYS.contains(&"sync.upload_stats"));
    }

    /// Helper: call `set_parsed_field` for `history.retention`.
    fn set_retention(path: &std::path::Path, value: &str) -> i32 {
        set_parsed_field(
            path,
            "history.retention",
            value,
            "a non-negative integer",
            |cfg, n| {
                cfg.history
                    .get_or_insert(history::TokfHistorySection { retention: None })
                    .retention = Some(n);
            },
        )
    }

    /// Helper: call `set_parsed_field` for `sync.auto_sync_threshold`.
    fn set_sync_threshold(path: &std::path::Path, value: &str) -> i32 {
        set_parsed_field(
            path,
            "sync.auto_sync_threshold",
            value,
            "a non-negative integer",
            |cfg, n| {
                cfg.sync
                    .get_or_insert(TokfSyncSection {
                        auto_sync_threshold: None,
                        upload_usage_stats: None,
                    })
                    .auto_sync_threshold = Some(n);
            },
        )
    }

    #[test]
    fn set_retention_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(set_retention(&path, "25"), 0);
        let cfg = load_project_config(&path);
        assert_eq!(cfg.history.unwrap().retention, Some(25));
    }

    #[test]
    fn set_retention_invalid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(set_retention(&path, "abc"), 1);
    }

    #[test]
    fn set_sync_threshold_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(set_sync_threshold(&path, "50"), 0);
        let cfg = load_project_config(&path);
        assert_eq!(cfg.sync.unwrap().auto_sync_threshold, Some(50));
    }

    #[test]
    fn set_upload_stats_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(set_upload_stats(&path, "true"), 0);
        let cfg = load_project_config(&path);
        assert_eq!(cfg.sync.unwrap().upload_usage_stats, Some(true));
    }

    #[test]
    fn set_upload_stats_invalid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(set_upload_stats(&path, "yes"), 1);
    }

    #[test]
    fn set_shims_enabled_valid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(
            set_parsed_field(
                &path,
                "shims.enabled",
                "false",
                "true or false",
                |cfg, b| {
                    cfg.shims
                        .get_or_insert(TokfShimsSection { enabled: None })
                        .enabled = Some(b);
                }
            ),
            0
        );
        let cfg = load_project_config(&path);
        assert_eq!(cfg.shims.unwrap().enabled, Some(false));
    }

    #[test]
    fn set_shims_enabled_invalid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(
            set_parsed_field(&path, "shims.enabled", "yes", "true or false", |cfg, b| {
                cfg.shims
                    .get_or_insert(TokfShimsSection { enabled: None })
                    .enabled = Some(b);
            }),
            1
        );
    }

    #[test]
    fn collect_config_entries_shims_default() {
        let dir = TempDir::new().unwrap();
        let local = dir.path().join(".tokf/config.toml");
        let entries = collect_config_entries(None, &local, dir.path());
        let shims_entry = entries.iter().find(|e| e.key == "shims.enabled").unwrap();
        assert_eq!(shims_entry.value.as_deref(), Some("true"));
        assert_eq!(shims_entry.source, "default");
    }

    #[test]
    fn set_preserves_existing_fields() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[history]\nretention = 30\n").unwrap();

        set_sync_threshold(&path, "200");

        let cfg = load_project_config(&path);
        assert_eq!(cfg.history.unwrap().retention, Some(30));
        assert_eq!(cfg.sync.unwrap().auto_sync_threshold, Some(200));
    }
}
