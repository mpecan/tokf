use std::collections::HashSet;
use std::path::{Path, PathBuf};

// --- All-filter coverage discovery (for --require-all) ---

pub(super) fn collect_all_filters(
    root: &Path,
    dir: &Path,
    result: &mut Vec<(String, bool)>,
    seen: &mut HashSet<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in &entries {
        let path = entry.path();
        let name_str = entry.file_name().to_string_lossy().to_string();
        if name_str.starts_with('.') {
            continue;
        }
        if path.is_file() && path.extension().is_some_and(|e| e == "toml") {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let suite_dir = path.parent().unwrap_or(dir).join(format!("{stem}_test"));
            let filter_name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            #[cfg(windows)]
            let filter_name = filter_name.replace('\\', "/");
            if seen.insert(filter_name.clone()) {
                result.push((filter_name, suite_dir.is_dir()));
            }
        } else if path.is_dir() && !name_str.ends_with("_test") {
            collect_all_filters(root, &path, result, seen);
        }
    }
}

pub(super) fn discover_all_filters_with_coverage(
    search_dirs: &[PathBuf],
    prefix: Option<&str>,
) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        collect_all_filters(dir, dir, &mut result, &mut seen);
    }
    if let Some(pfx) = prefix {
        result.retain(|(name, _)| name == pfx || name.starts_with(&format!("{pfx}/")));
    }
    result
}

// --- Search dirs for verify ---

// Intentionally different from `config::default_search_dirs()`: verify puts
// `filters/` (stdlib) first so repo developers test the stdlib by default,
// while the runtime puts `.tokf/filters/` (project overrides) first.
pub(super) fn verify_search_dirs(scope: Option<&super::VerifyScope>) -> Vec<PathBuf> {
    match scope {
        Some(super::VerifyScope::Project) => {
            let mut dirs = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                dirs.push(cwd.join(".tokf/filters"));
            }
            dirs
        }
        Some(super::VerifyScope::Global) => {
            let mut dirs = Vec::new();
            if let Some(config) = dirs::config_dir() {
                dirs.push(config.join("tokf/filters"));
            }
            dirs
        }
        Some(super::VerifyScope::Stdlib) => {
            let mut dirs = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                dirs.push(cwd.join("filters"));
            }
            dirs
        }
        None => {
            // Priority order (highest first):
            //   1. filters/ in CWD — catches the stdlib during repo development
            //   2. .tokf/filters/ in CWD — repo-local custom filters
            //   3. {config_dir}/tokf/filters/ — user-level custom filters
            // When the same filter name appears in multiple dirs, the first wins.
            let mut dirs = Vec::new();
            if let Ok(cwd) = std::env::current_dir() {
                dirs.push(cwd.join("filters"));
                dirs.push(cwd.join(".tokf/filters"));
            }
            if let Some(config) = dirs::config_dir() {
                dirs.push(config.join("tokf/filters"));
            }
            dirs
        }
    }
}

// --- Suite discovery ---

/// A discovered suite: filter TOML path, suite directory, and filter name.
pub(super) struct DiscoveredSuite {
    pub filter_path: PathBuf,
    pub suite_dir: PathBuf,
    pub filter_name: String,
}

pub(super) fn discover_suites(
    search_dirs: &[PathBuf],
    filter_arg: Option<&str>,
) -> Vec<DiscoveredSuite> {
    let mut result = Vec::new();

    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        collect_suites(dir, dir, &mut result);
    }

    // Remove duplicates: prefer first occurrence (higher priority dir).
    // HashSet tracks seen names; retain() preserves insertion order.
    let mut seen = HashSet::new();
    result.retain(|s| seen.insert(s.filter_name.clone()));

    if let Some(name) = filter_arg {
        result.retain(|s| s.filter_name == name);
    }

    result
}

fn collect_suites(root: &Path, dir: &Path, result: &mut Vec<DiscoveredSuite>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in &entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if path.is_file() && path.extension().is_some_and(|e| e == "toml") {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Suite directories use the convention <stem>_test/ adjacent to <stem>.toml.
            let suite_dir = path.parent().unwrap_or(dir).join(format!("{stem}_test"));
            if suite_dir.is_dir() {
                let filter_name = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .with_extension("")
                    .to_string_lossy()
                    .into_owned();
                // Normalize path separators on Windows so filter names are always "foo/bar".
                #[cfg(windows)]
                let filter_name = filter_name.replace('\\', "/");
                result.push(DiscoveredSuite {
                    filter_path: path,
                    suite_dir,
                    filter_name,
                });
            }
        } else if path.is_dir() {
            // Skip _test directories — they are suite dirs, not filter category dirs.
            if name_str.ends_with("_test") {
                continue;
            }
            collect_suites(root, &path, result);
        }
    }
}
