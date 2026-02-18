pub mod types;

use std::path::{Path, PathBuf};

use anyhow::Context;

use types::FilterConfig;

/// Derive filter filename from command words.
/// `["git", "push"]` → `"git-push.toml"`, `["cargo", "test"]` → `"cargo-test.toml"`
pub fn command_to_filter_name(command_words: &[&str]) -> String {
    format!("{}.toml", command_words.join("-"))
}

/// Build default search dirs in priority order:
/// 1. `.tokf/filters/` (repo-local, resolved from CWD)
/// 2. `{config_dir}/tokf/filters/` (user-level, platform-native)
/// 3. `{binary_dir}/filters/` (shipped stdlib, adjacent to the tokf binary)
pub fn default_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // 1. Repo-local override (resolved to absolute so it survives any later CWD change)
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(".tokf/filters"));
    }

    // 2. User-level config dir (platform-native)
    if let Some(config) = dirs::config_dir() {
        dirs.push(config.join("tokf/filters"));
    }

    // 3. Binary-adjacent stdlib
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin_dir) = exe.parent()
    {
        dirs.push(bin_dir.join("filters"));
    }

    dirs
}

/// Try to load a filter from `path`. Returns `Ok(Some(config))` on success,
/// `Ok(None)` if the file does not exist, or `Err` for other I/O / parse errors.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or contains invalid TOML.
pub fn try_load_filter(path: &Path) -> anyhow::Result<Option<FilterConfig>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::Error::new(e)
                .context(format!("failed to read filter file: {}", path.display())));
        }
    };
    let config: FilterConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse filter file: {}", path.display()))?;
    Ok(Some(config))
}

/// Read a directory and return sorted TOML filter file entries.
///
/// Returns entries sorted by filename for deterministic ordering.
/// Silently returns an empty vec if the directory doesn't exist or can't be read.
pub fn sorted_filter_files(dir: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut toml_files: Vec<_> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    toml_files.sort_by_key(std::fs::DirEntry::file_name);
    toml_files
}

/// Core resolution with injectable search dirs (for testing).
///
/// # Errors
///
/// Returns an error if a matching filter file is found but cannot be read or parsed.
pub fn resolve_filter_in(
    command_words: &[&str],
    search_dirs: &[PathBuf],
) -> anyhow::Result<Option<FilterConfig>> {
    if command_words.is_empty() {
        eprintln!("[tokf] no filter for \"\", passing through");
        return Ok(None);
    }

    let filter_name = command_to_filter_name(command_words);

    for dir in search_dirs {
        let candidate = dir.join(&filter_name);
        if let Some(config) = try_load_filter(&candidate)? {
            return Ok(Some(config));
        }
    }

    let command_str = command_words.join(" ");
    eprintln!("[tokf] no filter for \"{command_str}\", passing through");
    Ok(None)
}

/// Resolve a filter config for the given command.
/// Searches dirs in priority order, returns `Ok(None)` with stderr note if not found.
///
/// # Errors
///
/// Returns an error if a matching filter file is found but cannot be read or parsed.
pub fn resolve_filter(command_words: &[&str]) -> anyhow::Result<Option<FilterConfig>> {
    resolve_filter_in(command_words, &default_search_dirs())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    // --- command_to_filter_name ---

    #[test]
    fn test_two_words() {
        assert_eq!(command_to_filter_name(&["git", "push"]), "git-push.toml");
    }

    #[test]
    fn test_single_word() {
        assert_eq!(command_to_filter_name(&["rustfmt"]), "rustfmt.toml");
    }

    #[test]
    fn test_three_words() {
        assert_eq!(
            command_to_filter_name(&["cargo", "test", "unit"]),
            "cargo-test-unit.toml"
        );
    }

    #[test]
    fn test_empty() {
        assert_eq!(command_to_filter_name(&[]), ".toml");
    }

    // --- try_load_filter ---

    #[test]
    fn test_load_valid_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.toml");
        fs::write(&path, "command = \"echo hello\"").unwrap();

        let config = try_load_filter(&path).unwrap().unwrap();
        assert_eq!(config.command, "echo hello");
    }

    #[test]
    fn test_load_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "not valid toml [[[").unwrap();

        assert!(try_load_filter(&path).is_err());
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let path = PathBuf::from("/tmp/nonexistent-tokf-test-file.toml");
        assert!(try_load_filter(&path).unwrap().is_none());
    }

    #[test]
    fn test_load_real_stdlib_filter() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters/git-push.toml");
        let config = try_load_filter(&path).unwrap().unwrap();
        assert_eq!(config.command, "git push");
    }

    // --- resolve_filter_in ---

    #[test]
    fn test_resolve_finds_in_first_dir() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        fs::write(dir1.path().join("git-push.toml"), "command = \"git push\"").unwrap();
        fs::write(
            dir2.path().join("git-push.toml"),
            "command = \"git push alt\"",
        )
        .unwrap();

        let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let config = resolve_filter_in(&["git", "push"], &dirs).unwrap().unwrap();
        assert_eq!(config.command, "git push");
    }

    #[test]
    fn test_resolve_falls_through() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        fs::write(dir2.path().join("git-push.toml"), "command = \"git push\"").unwrap();

        let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let config = resolve_filter_in(&["git", "push"], &dirs).unwrap().unwrap();
        assert_eq!(config.command, "git push");
    }

    #[test]
    fn test_resolve_returns_none_when_not_found() {
        let dir1 = TempDir::new().unwrap();
        let dirs = vec![dir1.path().to_path_buf()];
        let result = resolve_filter_in(&["nonexistent", "cmd"], &dirs).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_returns_error_on_invalid_toml() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("bad-cmd.toml"), "not valid [[[").unwrap();

        let dirs = vec![dir.path().to_path_buf()];
        assert!(resolve_filter_in(&["bad", "cmd"], &dirs).is_err());
    }

    #[test]
    fn test_resolve_empty_command_returns_none() {
        let dir = TempDir::new().unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        let result = resolve_filter_in(&[], &dirs).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_skips_nonexistent_search_dir() {
        let real_dir = TempDir::new().unwrap();
        fs::write(
            real_dir.path().join("git-push.toml"),
            "command = \"git push\"",
        )
        .unwrap();

        let dirs = vec![
            PathBuf::from("/no/such/directory/ever"),
            real_dir.path().to_path_buf(),
        ];
        let config = resolve_filter_in(&["git", "push"], &dirs).unwrap().unwrap();
        assert_eq!(config.command, "git push");
    }

    #[test]
    fn test_resolve_invalid_toml_in_first_dir_does_not_fall_through() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        fs::write(dir1.path().join("git-push.toml"), "not valid [[[").unwrap();
        fs::write(dir2.path().join("git-push.toml"), "command = \"git push\"").unwrap();

        let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        assert!(resolve_filter_in(&["git", "push"], &dirs).is_err());
    }

    // --- default_search_dirs ---

    #[test]
    fn test_default_search_dirs_non_empty_and_starts_with_local() {
        let dirs = default_search_dirs();
        assert!(!dirs.is_empty());
        // First entry should be CWD-resolved .tokf/filters (absolute path)
        assert!(
            dirs[0].is_absolute(),
            "first dir should be absolute, got: {:?}",
            dirs[0]
        );
        assert!(
            dirs[0].ends_with(".tokf/filters"),
            "first dir should end with .tokf/filters, got: {:?}",
            dirs[0]
        );
    }
}
