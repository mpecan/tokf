//! Centralised tokf user-directory resolution.
//!
//! When `TOKF_HOME` is set, it replaces **all** platform-native user directories
//! (config, data, cache).  Project-local `.tokf/` directories are unaffected.
//!
//! Priority for the user-level base directory:
//!   1. `TOKF_HOME` env var (if set and non-empty)
//!   2. `dirs::config_dir().map(|d| d.join("tokf"))` (platform default)
//!
//! For the tracking database, an additional override applies on top:
//!   1. `TOKF_DB_PATH` env var  (highest priority, unchanged from before)
//!   2. `TOKF_HOME`             (if set)
//!   3. `dirs::data_local_dir().map(|d| d.join("tokf"))`

use std::path::PathBuf;

/// Shared resolution logic: return the `TOKF_HOME` path when set and non-empty,
/// otherwise fall through to the platform-native `dirs_fallback`.
fn resolve_user_path(dirs_fallback: Option<PathBuf>) -> Option<PathBuf> {
    if let Ok(home) = std::env::var("TOKF_HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    dirs_fallback
}

/// Returns the tokf user-level base directory.
///
/// When `TOKF_HOME` is set (and non-empty), returns that path directly.
/// Otherwise returns `dirs::config_dir().map(|d| d.join("tokf"))`.
///
/// This is the single source of truth for all config-like user paths:
/// `filters/`, `rewrites.toml`, `machine.toml`, `auth.toml`, `config.toml`, `hooks/`.
pub fn user_dir() -> Option<PathBuf> {
    resolve_user_path(dirs::config_dir().map(|d| d.join("tokf")))
}

/// Returns the base directory for data files (tracking DB).
///
/// When `TOKF_HOME` is set, identical to `user_dir()`.
/// Otherwise falls back to `dirs::data_local_dir().map(|d| d.join("tokf"))`.
///
/// Callers that also respect `TOKF_DB_PATH` must check that env var first.
pub fn user_data_dir() -> Option<PathBuf> {
    resolve_user_path(dirs::data_local_dir().map(|d| d.join("tokf")))
}

/// Returns the base directory for cache files (filter manifest).
///
/// When `TOKF_HOME` is set, identical to `user_dir()`.
/// Otherwise falls back to `dirs::cache_dir().map(|d| d.join("tokf"))`.
pub fn user_cache_dir() -> Option<PathBuf> {
    resolve_user_path(dirs::cache_dir().map(|d| d.join("tokf")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn set_tokf_home(val: &str) {
        // SAFETY: test-only env mutation; #[serial] prevents races.
        unsafe { std::env::set_var("TOKF_HOME", val) };
    }

    fn clear_tokf_home() {
        unsafe { std::env::remove_var("TOKF_HOME") };
    }

    #[test]
    #[serial]
    fn user_dir_uses_tokf_home_when_set() {
        set_tokf_home("/custom/tokf/home");
        let result = user_dir();
        clear_tokf_home();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_dir_ignores_empty_tokf_home() {
        set_tokf_home("");
        let result = user_dir();
        clear_tokf_home();
        // Should fall back to dirs::config_dir() — just verify it's not an empty path.
        if let Some(p) = result {
            assert_ne!(p, PathBuf::from(""));
        }
    }

    #[test]
    #[serial]
    fn user_data_dir_uses_tokf_home_when_set() {
        set_tokf_home("/custom/tokf/home");
        let result = user_data_dir();
        clear_tokf_home();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_cache_dir_uses_tokf_home_when_set() {
        set_tokf_home("/custom/tokf/home");
        let result = user_cache_dir();
        clear_tokf_home();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_dir_fallback_matches_dirs_crate() {
        clear_tokf_home();
        let via_paths = user_dir();
        let via_dirs = dirs::config_dir().map(|d| d.join("tokf"));
        assert_eq!(via_paths, via_dirs);
    }

    #[test]
    #[serial]
    fn all_three_dirs_agree_when_tokf_home_set() {
        set_tokf_home("/unified/home");
        let config = user_dir();
        let data = user_data_dir();
        let cache = user_cache_dir();
        clear_tokf_home();
        assert_eq!(config, data);
        assert_eq!(data, cache);
        assert_eq!(config, Some(PathBuf::from("/unified/home")));
    }
}
