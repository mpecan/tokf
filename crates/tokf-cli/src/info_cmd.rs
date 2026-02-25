use std::path::{Path, PathBuf};

use serde::Serialize;

use tokf::config;
use tokf::tracking;

/// Write-access status for a path that tokf needs to write to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum WriteAccess {
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
    const fn label(self) -> &'static str {
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
/// Uses `create_new(true)` to avoid following a pre-existing symlink (TOCTOU mitigation).
fn is_writable(path: &Path) -> bool {
    if path.is_file() {
        std::fs::OpenOptions::new().write(true).open(path).is_ok()
    } else if path.is_dir() {
        let probe = path.join(".tokf_write_check");
        // create_new(true) fails atomically if the path already exists, preventing
        // a symlink from being followed to an attacker-controlled target.
        let ok = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .is_ok();
        if ok {
            let _ = std::fs::remove_file(&probe);
        }
        ok
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
struct SearchDir {
    scope: &'static str,
    path: String,
    exists: bool,
    /// `Some(access)` when the directory exists; `None` when it does not.
    access: Option<WriteAccess>,
}

#[derive(Serialize)]
struct TrackingDb {
    env_override: Option<String>,
    path: Option<String>,
    exists: bool,
    access: Option<WriteAccess>,
}

#[derive(Serialize)]
struct CacheInfo {
    path: Option<String>,
    exists: bool,
    access: Option<WriteAccess>,
}

#[derive(Serialize)]
struct FilterCounts {
    local: usize,
    user: usize,
    builtin: usize,
    total: usize,
}

#[derive(Serialize)]
struct InfoOutput {
    version: String,
    /// `TOKF_HOME` env var value when set; affects all user-level paths.
    home_override: Option<String>,
    search_dirs: Vec<SearchDir>,
    tracking_db: TrackingDb,
    cache: CacheInfo,
    filters: Option<FilterCounts>,
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

fn collect_filter_counts(search_dirs: &[PathBuf]) -> Option<FilterCounts> {
    match config::discover_all_filters(search_dirs) {
        Ok(f) => {
            let local = f.iter().filter(|fi| fi.priority == 0).count();
            let user = f
                .iter()
                .filter(|fi| fi.priority > 0 && fi.priority < u8::MAX)
                .count();
            let builtin = f.iter().filter(|fi| fi.priority == u8::MAX).count();
            Some(FilterCounts {
                local,
                user,
                builtin,
                total: f.len(),
            })
        }
        Err(e) => {
            eprintln!("[tokf] error discovering filters: {e:#}");
            None
        }
    }
}

fn collect_info(search_dirs: &[PathBuf]) -> InfoOutput {
    let dirs = collect_search_dirs(search_dirs);

    let home_override = std::env::var("TOKF_HOME").ok();
    let env_override = std::env::var("TOKF_DB_PATH").ok();
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

    InfoOutput {
        version: env!("CARGO_PKG_VERSION").to_string(),
        home_override,
        search_dirs: dirs,
        tracking_db,
        cache,
        filters: collect_filter_counts(search_dirs),
    }
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
