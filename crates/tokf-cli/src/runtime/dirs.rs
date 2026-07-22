//! Fully-resolved user directories.
//!
//! Unlike the process-global `paths` module this replaces, a [`Dirs`] value is
//! resolved **once**, at construction. Nothing here consults the environment or
//! any static, so two `Dirs` values are completely independent — which is what
//! lets two tests run concurrently against different directories.

use std::path::PathBuf;

/// Resolved user-level directories plus the optional tracking-database override.
///
/// Each field is already the final answer: `TOKF_HOME` (when set) has replaced
/// the platform-native directory, and the platform fallbacks from the `dirs`
/// crate have been evaluated. `None` means the platform could not supply a
/// directory at all — the same condition `dirs::config_dir()` reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dirs {
    /// Base directory for config-like files: `filters/`, `rewrites.toml`,
    /// `machine.toml`, `auth.toml`, `config.toml`, `hooks/`.
    pub(super) config: Option<PathBuf>,
    /// Base directory for data files (the tracking database).
    pub(super) data: Option<PathBuf>,
    /// Base directory for cache files (filter manifest, shims).
    pub(super) cache: Option<PathBuf>,
    /// Explicit `TOKF_DB_PATH` override, if any. Takes priority over `data`.
    pub(super) db_path: Option<PathBuf>,
    /// The raw `TOKF_HOME` value, retained purely so `tokf info` can report it.
    pub(super) home_override: Option<PathBuf>,
}

impl Dirs {
    /// Build directories rooted at `home`, mirroring what `TOKF_HOME` does:
    /// config, data and cache all collapse to the same directory.
    pub fn rooted_at(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            config: Some(home.clone()),
            data: Some(home.clone()),
            cache: Some(home.clone()),
            db_path: None,
            home_override: Some(home),
        }
    }

    /// The tokf user-level base directory (config-like files).
    pub fn user_dir(&self) -> Option<PathBuf> {
        self.config.clone()
    }

    /// The base directory for data files.
    pub fn user_data_dir(&self) -> Option<PathBuf> {
        self.data.clone()
    }

    /// The base directory for cache files.
    pub fn user_cache_dir(&self) -> Option<PathBuf> {
        self.cache.clone()
    }

    /// The directory for generated shim scripts, `<cache>/shims/`.
    pub fn shims_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|d| d.join("shims"))
    }

    /// The tracking database path.
    ///
    /// Priority: `TOKF_DB_PATH` → `<data>/tracking.db`. This is the same order
    /// the former `tracking::db_path()` applied, with `TOKF_HOME` already folded
    /// into `data` at construction.
    pub fn tracking_db_path(&self) -> Option<PathBuf> {
        self.db_path
            .clone()
            .or_else(|| self.data.as_ref().map(|d| d.join("tracking.db")))
    }

    /// The raw `TOKF_HOME` override, for `tokf info` reporting only.
    pub fn home_override(&self) -> Option<&std::path::Path> {
        self.home_override.as_deref()
    }

    /// The raw `TOKF_DB_PATH` override, for `tokf info` reporting only.
    pub fn db_path_override(&self) -> Option<&std::path::Path> {
        self.db_path.as_deref()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn rooted_at_collapses_all_three_directories() {
        let dirs = Dirs::rooted_at("/unified/home");
        assert_eq!(dirs.user_dir(), Some(PathBuf::from("/unified/home")));
        assert_eq!(dirs.user_data_dir(), dirs.user_dir());
        assert_eq!(dirs.user_cache_dir(), dirs.user_dir());
    }

    #[test]
    fn shims_dir_hangs_off_the_cache_directory() {
        let dirs = Dirs::rooted_at("/custom/tokf/home");
        assert_eq!(
            dirs.shims_dir(),
            Some(PathBuf::from("/custom/tokf/home/shims"))
        );
    }

    #[test]
    fn shims_dir_is_none_without_a_cache_directory() {
        assert_eq!(Dirs::default().shims_dir(), None);
    }

    #[test]
    fn tracking_db_path_defaults_under_the_data_directory() {
        let dirs = Dirs::rooted_at("/home");
        assert_eq!(
            dirs.tracking_db_path(),
            Some(PathBuf::from("/home/tracking.db"))
        );
    }

    #[test]
    fn tracking_db_path_override_wins_over_the_data_directory() {
        let mut dirs = Dirs::rooted_at("/home");
        dirs.db_path = Some(PathBuf::from("/elsewhere/custom.sqlite"));
        assert_eq!(
            dirs.tracking_db_path(),
            Some(PathBuf::from("/elsewhere/custom.sqlite"))
        );
    }

    #[test]
    fn tracking_db_path_is_none_when_nothing_is_resolvable() {
        assert_eq!(Dirs::default().tracking_db_path(), None);
    }
}
