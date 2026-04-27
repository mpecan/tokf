use std::path::{Path, PathBuf};

use serde::Serialize;

use tokf::config::{self, ResolvedFilter};
use tokf::tracking;

/// Write-access status for a path that tokf needs to write to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteAccess {
    /// Path exists and is writable by the current process.
    Writable,
    /// Path exists but is not writable.
    ReadOnly,
    /// Path does not exist; its nearest existing ancestor is writable (will be auto-created).
    WillCreate,
    /// Path does not exist and its nearest existing ancestor is not writable.
    ParentReadOnly,
    /// Neither the path nor any ancestor could be found or checked.
    Unavailable,
}

impl WriteAccess {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Writable => "writable",
            Self::ReadOnly => "read-only!",
            Self::WillCreate => "will be created",
            Self::ParentReadOnly => "dir not writable!",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Returns `true` if the current process can write to `path`.
/// For directories, briefly creates and removes a probe file to test access accurately.
///
/// Uses `create_new(true)` to avoid following a pre-existing symlink (TOCTOU mitigation).
/// A PID-suffixed probe name with up to 5 attempts handles the rare case where a previous
/// probe file with the same name already exists, avoiding false negatives.
fn is_writable(path: &Path) -> bool {
    if path.is_file() {
        std::fs::OpenOptions::new().write(true).open(path).is_ok()
    } else if path.is_dir() {
        let pid = std::process::id();
        for attempt in 0..5u32 {
            let probe = path.join(format!(".tokf_write_check_{pid}_{attempt}"));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
            {
                Ok(_) => {
                    let _ = std::fs::remove_file(&probe);
                    return true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return false,
            }
        }
        false
    } else {
        false
    }
}

/// Determine write-access status for `path`, walking up to the nearest existing ancestor
/// when the path itself does not exist.
fn check_write_access(path: &Path) -> WriteAccess {
    if path.exists() {
        return if is_writable(path) {
            WriteAccess::Writable
        } else {
            WriteAccess::ReadOnly
        };
    }
    let mut ancestor = path.parent();
    while let Some(a) = ancestor {
        if a.exists() {
            return if is_writable(a) {
                WriteAccess::WillCreate
            } else {
                WriteAccess::ParentReadOnly
            };
        }
        ancestor = a.parent();
    }
    WriteAccess::Unavailable
}

#[derive(Serialize)]
pub struct SearchDir {
    pub scope: &'static str,
    pub path: String,
    pub exists: bool,
    /// `Some(access)` when the directory exists; `None` when it does not.
    pub access: Option<WriteAccess>,
}

#[derive(Serialize)]
pub struct TrackingDb {
    pub env_override: Option<String>,
    pub path: Option<String>,
    pub exists: bool,
    pub access: Option<WriteAccess>,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub path: Option<String>,
    pub exists: bool,
    pub access: Option<WriteAccess>,
}

#[derive(Serialize)]
pub struct FilterCounts {
    pub local: usize,
    pub user: usize,
    pub builtin: usize,
    pub total: usize,
}

#[derive(Serialize)]
pub struct ConfigFileEntry {
    pub scope: &'static str,
    pub path: String,
    pub exists: bool,
}

#[derive(Serialize)]
pub struct InfoOutput {
    pub version: String,
    /// `TOKF_HOME` env var value when set; affects all user-level paths.
    pub home_override: Option<String>,
    pub search_dirs: Vec<SearchDir>,
    pub tracking_db: TrackingDb,
    pub cache: CacheInfo,
    pub config_files: Vec<ConfigFileEntry>,
    pub filters: Option<FilterCounts>,
}

pub fn cmd_info(json: bool) -> i32 {
    let search_dirs = config::default_search_dirs();
    let info = collect_info(&search_dirs);

    if json {
        crate::output::print_json(&info);
    } else {
        print_human(&info);
    }
    0
}

fn collect_search_dirs(search_dirs: &[PathBuf]) -> Vec<SearchDir> {
    let mut dirs: Vec<SearchDir> = search_dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| SearchDir {
            scope: if i == 0 { "local" } else { "user" },
            path: dir.display().to_string(),
            exists: dir.exists(),
            access: dir.exists().then(|| {
                if is_writable(dir) {
                    WriteAccess::Writable
                } else {
                    WriteAccess::ReadOnly
                }
            }),
        })
        .collect();
    dirs.push(SearchDir {
        scope: "built-in",
        path: "<embedded>".to_string(),
        exists: true,
        access: None,
    });
    dirs
}

/// Bucket a discovered filter list into the counts shown in `tokf info`.
pub fn count_filters_by_priority(filters: &[ResolvedFilter]) -> FilterCounts {
    let local = filters.iter().filter(|fi| fi.priority == 0).count();
    let user = filters
        .iter()
        .filter(|fi| fi.priority > 0 && fi.priority < u8::MAX)
        .count();
    let builtin = filters.iter().filter(|fi| fi.priority == u8::MAX).count();
    FilterCounts {
        local,
        user,
        builtin,
        total: filters.len(),
    }
}

pub fn collect_info(search_dirs: &[PathBuf]) -> InfoOutput {
    let filters = match config::discover_all_filters(search_dirs) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("[tokf] error discovering filters: {e:#}");
            None
        }
    };
    collect_info_with_filters(search_dirs, filters.as_deref())
}

/// Build an `InfoOutput` from a pre-discovered filter list. Pass `None` when
/// discovery failed; the caller is expected to have already logged the error.
pub fn collect_info_with_filters(
    search_dirs: &[PathBuf],
    filters: Option<&[ResolvedFilter]>,
) -> InfoOutput {
    let dirs = collect_search_dirs(search_dirs);

    // Normalise to None when empty/whitespace-only, matching paths::resolve_user_path() behaviour.
    let home_override = std::env::var("TOKF_HOME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let env_override = tokf::paths::db_path_override().map(|p| p.display().to_string());
    let db_path = tracking::db_path();
    let db_exists = db_path.as_ref().is_some_and(|p| p.exists());
    let db_access = db_path.as_ref().map(|p| check_write_access(p));
    let tracking_db = TrackingDb {
        env_override,
        path: db_path.map(|p| p.display().to_string()),
        exists: db_exists,
        access: db_access,
    };

    let cache_path = config::cache::cache_path(search_dirs);
    let cache_exists = cache_path.as_ref().is_some_and(|p| p.exists());
    let cache_access = cache_path.as_ref().map(|p| check_write_access(p));
    let cache = CacheInfo {
        path: cache_path.map(|p| p.display().to_string()),
        exists: cache_exists,
        access: cache_access,
    };

    let config_files = collect_config_files();

    InfoOutput {
        version: env!("CARGO_PKG_VERSION").to_string(),
        home_override,
        search_dirs: dirs,
        tracking_db,
        cache,
        config_files,
        filters: filters.map(count_filters_by_priority),
    }
}

fn collect_config_files() -> Vec<ConfigFileEntry> {
    let user_dir = tokf::paths::user_dir();
    let cwd = std::env::current_dir().unwrap_or_default();
    let project_root = tokf::history::project_root_for(&cwd);
    let local_dir = project_root.join(".tokf");

    let mut entries = Vec::new();

    // Global config files
    let global_files = ["config.toml", "auth.toml", "machine.toml", "rewrites.toml"];
    for file in &global_files {
        let path = user_dir.as_ref().map(|d| d.join(file));
        let exists = path.as_ref().is_some_and(|p| p.exists());
        entries.push(ConfigFileEntry {
            scope: "global",
            path: path.map_or_else(|| "(unavailable)".to_string(), |p| p.display().to_string()),
            exists,
        });
    }

    // Local config files
    let local_files = ["config.toml", "rewrites.toml"];
    for file in &local_files {
        let path = local_dir.join(file);
        let exists = path.exists();
        entries.push(ConfigFileEntry {
            scope: "local",
            path: path.display().to_string(),
            exists,
        });
    }

    entries
}

fn print_human(info: &InfoOutput) {
    println!("tokf {}", info.version);
    match &info.home_override {
        Some(p) => println!("TOKF_HOME: {p}"),
        None => println!("TOKF_HOME: (not set)"),
    }

    println!("\nfilter search directories:");
    for dir in &info.search_dirs {
        if dir.scope == "built-in" {
            println!("  [{}] {} (always available)", dir.scope, dir.path);
        } else {
            let status = if dir.exists {
                match dir.access {
                    Some(WriteAccess::Writable) => "exists, writable",
                    Some(WriteAccess::ReadOnly) => "exists, read-only!",
                    _ => "exists",
                }
            } else {
                "not found"
            };
            println!("  [{}] {} ({status})", dir.scope, dir.path);
        }
    }

    println!("\ntracking database:");
    match &info.tracking_db.env_override {
        Some(p) => println!("  TOKF_DB_PATH: {p}"),
        None => println!("  TOKF_DB_PATH: (not set)"),
    }
    match &info.tracking_db.path {
        Some(p) => {
            let status = info
                .tracking_db
                .access
                .map_or("unknown", WriteAccess::label);
            println!("  path: {p} ({status})");
        }
        None => println!("  path: (could not determine)"),
    }

    println!("\nfilter cache:");
    match &info.cache.path {
        Some(p) => {
            let status = info.cache.access.map_or("unknown", WriteAccess::label);
            println!("  path: {p} ({status})");
        }
        None => println!("  path: (could not determine)"),
    }

    println!("\nconfig files:");
    for entry in &info.config_files {
        let status = if entry.exists { "exists" } else { "not found" };
        println!("  [{}] {} ({status})", entry.scope, entry.path);
    }

    if let Some(f) = &info.filters {
        println!("\nfilters:");
        println!("  local:    {}", f.local);
        println!("  user:     {}", f.user);
        println!("  built-in: {}", f.builtin);
        println!("  total:    {}", f.total);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn is_writable_true_for_writable_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, b"hello").unwrap();
        assert!(is_writable(&file));
    }

    #[cfg(unix)]
    #[test]
    fn is_writable_false_for_readonly_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("ro.txt");
        std::fs::write(&file, b"hello").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(!is_writable(&file));
        // Restore so TempDir cleanup can remove the file.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn is_writable_true_for_writable_dir() {
        let dir = TempDir::new().unwrap();
        assert!(is_writable(dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn is_writable_false_for_readonly_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let ro_dir = tmp.path().join("ro_dir");
        std::fs::create_dir(&ro_dir).unwrap();
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!is_writable(&ro_dir));
        // Restore so cleanup works.
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn check_write_access_writable_for_existing_writable_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("test.db");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(check_write_access(&file), WriteAccess::Writable);
    }

    #[cfg(unix)]
    #[test]
    fn check_write_access_read_only_for_readonly_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("ro.db");
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(check_write_access(&file), WriteAccess::ReadOnly);
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn check_write_access_will_create_for_nonexistent_in_writable_dir() {
        let dir = TempDir::new().unwrap();
        // Path doesn't exist but its grandparent (the temp dir) is writable.
        let nonexistent = dir.path().join("subdir").join("new.db");
        assert_eq!(check_write_access(&nonexistent), WriteAccess::WillCreate);
    }

    #[cfg(unix)]
    #[test]
    fn check_write_access_parent_read_only_when_dir_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let ro_dir = tmp.path().join("ro");
        std::fs::create_dir(&ro_dir).unwrap();
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let nested = ro_dir.join("new.db");
        assert_eq!(check_write_access(&nested), WriteAccess::ParentReadOnly);
        std::fs::set_permissions(&ro_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
