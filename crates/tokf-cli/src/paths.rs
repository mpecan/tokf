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
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

/// Three-state override for path-based env vars (`TOKF_HOME`, `TOKF_DB_PATH`).
///
/// The three states are necessary because `init_from_env()` may run before or
/// after code that reads these values. `Unset` signals "not yet initialised —
/// fall back to `std::env::var()`", while `Empty` signals "initialised, but
/// the env var was absent or empty — use platform defaults".  Without the
/// `Unset` variant every call would either re-read the env var (slow) or
/// silently ignore env overrides set after startup.
enum PathOverride {
    /// Not initialized — fall back to `std::env::var()` at call site.
    Unset,
    /// Initialized, but no path provided — use platform dirs.
    Empty,
    /// Explicit path override.
    Set(PathBuf),
}

static HOME: Mutex<PathOverride> = Mutex::new(PathOverride::Unset);
static DB_PATH: Mutex<PathOverride> = Mutex::new(PathOverride::Unset);

/// `TOKF_DEBUG` flag: 0 = unset (fall back to env), 1 = off, 2 = on.
const DEBUG_UNSET: u8 = 0;
const DEBUG_OFF: u8 = 1;
const DEBUG_ON: u8 = 2;
static DEBUG: AtomicU8 = AtomicU8::new(DEBUG_UNSET);

/// Initialize `HOME`, `DB_PATH`, and `DEBUG` from environment variables.
///
/// Call once at the start of `main()`. After this, `resolve_user_path()`,
/// `db_path_override()`, and `debug_enabled()` read from process-global
/// state rather than re-reading env vars on every call.
pub fn init_from_env() {
    let home = std::env::var("TOKF_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    set_home(home);

    let db = std::env::var("TOKF_DB_PATH").ok().map(PathBuf::from);
    set_db_path(db);

    let debug = std::env::var("TOKF_DEBUG")
        .ok()
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    set_debug(debug);
}

/// Set the `TOKF_HOME` override.
///
/// - `Some(path)` — use `path` as the home directory.
/// - `None` — explicitly disable `TOKF_HOME` (use platform defaults).
///   This is distinct from [`reset_home`], which returns to the
///   uninitialized state where `std::env::var("TOKF_HOME")` is consulted.
///
/// Prefer [`HomeGuard`] in tests for automatic cleanup on panic.
pub fn set_home(path: Option<PathBuf>) {
    let mut guard = HOME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = path.map_or(PathOverride::Empty, PathOverride::Set);
}

/// Reset the `HOME` override to the uninitialized state.
///
/// After this call, [`user_dir`] and friends will consult
/// `std::env::var("TOKF_HOME")` again on each call, as if
/// [`init_from_env`] had never run.
pub fn reset_home() {
    let mut guard = HOME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = PathOverride::Unset;
}

/// Set the `TOKF_DB_PATH` override.
///
/// - `Some(path)` — use `path` as the database file path.
/// - `None` — explicitly disable `TOKF_DB_PATH` (use platform defaults).
///   This is distinct from [`reset_db_path`], which returns to the
///   uninitialized state where `std::env::var("TOKF_DB_PATH")` is consulted.
///
/// Prefer [`DbPathGuard`] in tests for automatic cleanup on panic.
pub fn set_db_path(path: Option<PathBuf>) {
    let mut guard = DB_PATH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = path.map_or(PathOverride::Empty, PathOverride::Set);
}

/// Reset the `DB_PATH` override to the uninitialized state.
///
/// After this call, [`db_path_override`] will consult
/// `std::env::var("TOKF_DB_PATH")` again on each call.
pub fn reset_db_path() {
    let mut guard = DB_PATH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = PathOverride::Unset;
}

/// Set the `TOKF_DEBUG` flag explicitly.
///
/// Prefer [`DebugGuard`] in tests for automatic cleanup on panic.
pub fn set_debug(enabled: bool) {
    DEBUG.store(
        if enabled { DEBUG_ON } else { DEBUG_OFF },
        Ordering::Relaxed,
    );
}

/// Reset the `TOKF_DEBUG` flag to the uninitialized state (fall back to env var).
pub fn reset_debug() {
    DEBUG.store(DEBUG_UNSET, Ordering::Relaxed);
}

/// Returns `true` when `TOKF_DEBUG` is enabled.
///
/// Checks the process-global flag first; falls back to `std::env::var` if
/// uninitialized (backward compat for code paths that run before `init_from_env`).
pub fn debug_enabled() -> bool {
    match DEBUG.load(Ordering::Relaxed) {
        DEBUG_ON => true,
        DEBUG_OFF => false,
        // Unset — fall back to env var
        _ => std::env::var("TOKF_DEBUG")
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
    }
}

/// Returns the `TOKF_DB_PATH` override if set, otherwise falls back to env var.
///
/// Returns `None` when neither the Mutex nor the env var provides a value.
pub fn db_path_override() -> Option<PathBuf> {
    let guard = DB_PATH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &*guard {
        PathOverride::Set(p) => return Some(p.clone()),
        PathOverride::Empty => return None,
        PathOverride::Unset => {}
    }
    drop(guard);
    std::env::var("TOKF_DB_PATH").ok().map(PathBuf::from)
}

/// Shared resolution logic: return the `TOKF_HOME` path when set and non-empty,
/// otherwise fall through to the platform-native `dirs_fallback`.
fn resolve_user_path(dirs_fallback: Option<PathBuf>) -> Option<PathBuf> {
    let guard = HOME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match &*guard {
        PathOverride::Set(p) => return Some(p.clone()),
        PathOverride::Empty => return dirs_fallback,
        PathOverride::Unset => {}
    }
    drop(guard);

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

// ---------------------------------------------------------------------------
// RAII guards for tests — guarantee cleanup even when assertions panic.
// ---------------------------------------------------------------------------

/// RAII guard that calls [`reset_home`] on drop.
///
/// Use [`HomeGuard::set`] for the common case of overriding `TOKF_HOME` to a
/// specific path, or [`HomeGuard::new`] to pass `None` (platform defaults).
#[cfg(any(test, feature = "test-keyring"))]
pub struct HomeGuard(());

#[cfg(any(test, feature = "test-keyring"))]
impl HomeGuard {
    /// Set the `TOKF_HOME` override and return a guard that resets it on drop.
    pub fn new(path: Option<PathBuf>) -> Self {
        set_home(path);
        Self(())
    }

    /// Convenience: override `TOKF_HOME` to `path`.
    pub fn set(path: impl Into<PathBuf>) -> Self {
        Self::new(Some(path.into()))
    }
}

#[cfg(any(test, feature = "test-keyring"))]
impl Drop for HomeGuard {
    fn drop(&mut self) {
        reset_home();
    }
}

/// RAII guard that calls [`reset_db_path`] on drop.
#[cfg(any(test, feature = "test-keyring"))]
pub struct DbPathGuard(());

#[cfg(any(test, feature = "test-keyring"))]
impl DbPathGuard {
    /// Set the `TOKF_DB_PATH` override and return a guard that resets it on drop.
    pub fn new(path: Option<PathBuf>) -> Self {
        set_db_path(path);
        Self(())
    }

    /// Convenience: override `TOKF_DB_PATH` to `path`.
    pub fn set(path: impl Into<PathBuf>) -> Self {
        Self::new(Some(path.into()))
    }
}

#[cfg(any(test, feature = "test-keyring"))]
impl Drop for DbPathGuard {
    fn drop(&mut self) {
        reset_db_path();
    }
}

/// RAII guard that calls [`reset_debug`] on drop.
#[cfg(any(test, feature = "test-keyring"))]
pub struct DebugGuard(());

#[cfg(any(test, feature = "test-keyring"))]
impl DebugGuard {
    /// Set the `TOKF_DEBUG` flag and return a guard that resets it on drop.
    pub fn new(enabled: bool) -> Self {
        set_debug(enabled);
        Self(())
    }
}

#[cfg(any(test, feature = "test-keyring"))]
impl Drop for DebugGuard {
    fn drop(&mut self) {
        reset_debug();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serial_test::serial;

    use super::*;

    #[test]
    #[serial]
    fn user_dir_uses_tokf_home_when_set() {
        let _guard = HomeGuard::set("/custom/tokf/home");
        let result = user_dir();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_dir_ignores_empty_tokf_home() {
        let _guard = HomeGuard::new(None);
        let result = user_dir();
        // Should fall back to dirs::config_dir() — just verify it's not an empty path.
        if let Some(p) = result {
            assert_ne!(p, PathBuf::from(""));
        }
    }

    #[test]
    #[serial]
    fn user_data_dir_uses_tokf_home_when_set() {
        let _guard = HomeGuard::set("/custom/tokf/home");
        let result = user_data_dir();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_cache_dir_uses_tokf_home_when_set() {
        let _guard = HomeGuard::set("/custom/tokf/home");
        let result = user_cache_dir();
        assert_eq!(result, Some(PathBuf::from("/custom/tokf/home")));
    }

    #[test]
    #[serial]
    fn user_dir_fallback_matches_dirs_crate() {
        let _guard = HomeGuard::new(None);
        let via_paths = user_dir();
        let via_dirs = dirs::config_dir().map(|d| d.join("tokf"));
        assert_eq!(via_paths, via_dirs);
    }

    #[test]
    #[serial]
    fn all_three_dirs_agree_when_tokf_home_set() {
        let _guard = HomeGuard::set("/unified/home");
        let config = user_dir();
        let data = user_data_dir();
        let cache = user_cache_dir();
        assert_eq!(config, data);
        assert_eq!(data, cache);
        assert_eq!(config, Some(PathBuf::from("/unified/home")));
    }

    #[test]
    #[serial]
    fn db_path_override_returns_set_value() {
        let _guard = DbPathGuard::set("/custom/db.sqlite");
        let result = db_path_override();
        assert_eq!(result, Some(PathBuf::from("/custom/db.sqlite")));
    }

    #[test]
    #[serial]
    fn db_path_override_returns_none_when_cleared() {
        let _guard = DbPathGuard::new(None);
        let result = db_path_override();
        assert!(result.is_none());
    }
}
