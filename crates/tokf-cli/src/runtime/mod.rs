//! Explicit runtime configuration.
//!
//! Everything tokf reads from its environment — user directories, the tracking
//! database path, debug/telemetry flags, the server URL — lives in a single
//! [`Runtime`] value that is built once in `main()` and passed down by
//! reference.
//!
//! This module replaces the former `paths` module, which kept the same
//! information in process-global statics (`static HOME`, `static DB_PATH`,
//! `static DEBUG`) read at the point of use. That design meant a test could
//! only *mutate* the state production code read, so tests could not run
//! concurrently against different configurations, and a test that set nothing
//! silently used the developer's real config directory and keychain. Both
//! bugs in #422 came from that. With the statics gone there is no ambient
//! state to clobber: a `Runtime` is an ordinary value, two of them are
//! independent, and a function that needs configuration has to say so in its
//! signature.
//!
//! In tests, prefer [`Runtime::isolated`] (or `Runtime::default`, which is the
//! same thing over a temporary directory) — see `runtime/builder.rs`.

#[cfg(any(test, feature = "test-support"))]
mod builder;
mod dirs;
mod env;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub use dirs::Dirs;
pub use env::{
    DEFAULT_KEYRING_SERVICE, DEFAULT_SERVER_URL, DEFAULT_TIMEOUT_SECS, Flags, Net, OtelEnv,
};

#[cfg(any(test, feature = "test-support"))]
pub use builder::RuntimeBuilder;

/// Fully-resolved runtime configuration.
///
/// Construct with [`Runtime::from_env`] in production, or [`Runtime::isolated`]
/// in tests. Cheap enough to clone when a value must be moved into a thread.
#[derive(Debug, Clone)]
pub struct Runtime {
    dirs: Dirs,
    flags: Flags,
    net: Net,
    otel: OtelEnv,
    /// Working directory used to resolve project-local `.tokf/` paths.
    cwd: Option<PathBuf>,
    /// `TOKF_ORIGINAL_PATH` — the pre-shim `PATH`, set on nested invocations.
    original_path: Option<String>,
    /// `TOKF_CODEX_REWRITE_MODE`
    codex_rewrite_mode: Option<String>,
    /// Keyring service name. Constant in production; unique per instance in
    /// tests, so concurrent tests never collide in the shared mock store.
    keyring_service: String,
    /// Backing temporary directory for an isolated runtime, kept alive for as
    /// long as any clone of this value exists and removed when the last one is
    /// dropped. Always `None` in production.
    ///
    /// Held purely for that `Drop` side effect, so a non-test build never reads
    /// it — the accessor that does is `#[cfg]`-gated to test builds.
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(
            dead_code,
            reason = "kept alive for its Drop; only read in test builds"
        )
    )]
    temp_root: Option<Arc<tempfile::TempDir>>,
}

impl Runtime {
    // -- directories ------------------------------------------------------

    /// The tokf user-level base directory (`filters/`, `config.toml`, …).
    pub fn user_dir(&self) -> Option<PathBuf> {
        self.dirs.user_dir()
    }

    /// The base directory for data files (the tracking database).
    pub fn user_data_dir(&self) -> Option<PathBuf> {
        self.dirs.user_data_dir()
    }

    /// The base directory for cache files (filter manifest, shims).
    pub fn user_cache_dir(&self) -> Option<PathBuf> {
        self.dirs.user_cache_dir()
    }

    /// The directory for generated shim scripts.
    pub fn shims_dir(&self) -> Option<PathBuf> {
        self.dirs.shims_dir()
    }

    /// The tracking database path (`TOKF_DB_PATH` → `<data>/tracking.db`).
    pub fn tracking_db_path(&self) -> Option<PathBuf> {
        self.dirs.tracking_db_path()
    }

    /// The global `config.toml` path.
    pub fn global_config_path(&self) -> Option<PathBuf> {
        self.dirs.user_dir().map(|d| d.join("config.toml"))
    }

    /// The resolved directories, for callers that need several at once.
    pub const fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    /// Candidate locations for `relative`, in tokf's standard priority order:
    ///
    /// 1. `<cwd>/.tokf/<relative>` — project-local
    /// 2. `<user_dir>/<relative>` — user-level
    ///
    /// Entries whose base directory is unresolvable are omitted, so the result
    /// may be empty. Used for `filters/` and `rewrites.toml`.
    pub fn layered_paths(&self, relative: &str) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(2);
        if let Some(cwd) = self.cwd() {
            paths.push(cwd.join(".tokf").join(relative));
        }
        if let Some(user) = self.user_dir() {
            paths.push(user.join(relative));
        }
        paths
    }

    // -- flags ------------------------------------------------------------

    /// Whether `TOKF_DEBUG` is enabled.
    pub const fn debug(&self) -> bool {
        self.flags.debug
    }

    /// Whether `TOKF_VERBOSE` is enabled.
    pub const fn verbose(&self) -> bool {
        self.flags.verbose
    }

    /// Whether `TOKF_NO_FILTER` is enabled.
    pub const fn no_filter(&self) -> bool {
        self.flags.no_filter
    }

    /// The `TOKF_SHOW_INDICATOR` override, when set to a parseable boolean.
    pub const fn show_indicator(&self) -> Option<bool> {
        self.flags.show_indicator
    }

    // -- network ----------------------------------------------------------

    /// The tokf server URL.
    pub fn server_url(&self) -> &str {
        &self.net.server_url
    }

    /// The HTTP request timeout.
    pub const fn http_timeout(&self) -> Duration {
        self.net.http_timeout
    }

    // -- telemetry --------------------------------------------------------

    /// The raw OpenTelemetry environment.
    pub const fn otel(&self) -> &OtelEnv {
        &self.otel
    }

    // -- process ----------------------------------------------------------

    /// The working directory used to resolve project-local paths.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// The pre-shim `PATH` recorded by an outer tokf invocation, if any.
    pub fn original_path(&self) -> Option<&str> {
        self.original_path.as_deref()
    }

    /// The `TOKF_CODEX_REWRITE_MODE` setting, if any.
    pub fn codex_rewrite_mode(&self) -> Option<&str> {
        self.codex_rewrite_mode.as_deref()
    }

    /// The keyring service name credentials are stored under.
    pub fn keyring_service(&self) -> &str {
        &self.keyring_service
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn two_isolated_runtimes_share_no_paths() {
        let a = Runtime::isolated();
        let b = Runtime::isolated();

        assert_ne!(a.user_dir(), b.user_dir());
        assert_ne!(a.shims_dir(), b.shims_dir());
        assert_ne!(a.tracking_db_path(), b.tracking_db_path());
        // The keyring service is what partitions the shared mock store.
        assert_ne!(a.keyring_service(), b.keyring_service());
    }

    #[test]
    fn an_isolated_runtime_points_everything_inside_its_own_root() {
        let rt = Runtime::isolated();
        let root = rt.user_dir().unwrap();

        assert!(rt.user_data_dir().unwrap().starts_with(&root));
        assert!(rt.user_cache_dir().unwrap().starts_with(&root));
        assert!(rt.shims_dir().unwrap().starts_with(&root));
        assert!(rt.tracking_db_path().unwrap().starts_with(&root));
        assert!(rt.global_config_path().unwrap().starts_with(&root));
    }

    #[test]
    fn an_isolated_runtime_has_flags_off_and_no_otel() {
        let rt = Runtime::isolated();
        assert!(!rt.debug());
        assert!(!rt.verbose());
        assert!(!rt.no_filter());
        assert_eq!(rt.show_indicator(), None);
        assert_eq!(rt.otel(), &OtelEnv::default());
        assert_eq!(rt.original_path(), None);
    }

    #[test]
    fn global_config_path_sits_directly_under_the_user_dir() {
        let rt = Runtime::isolated();
        assert_eq!(
            rt.global_config_path().unwrap(),
            rt.user_dir().unwrap().join("config.toml")
        );
    }

    #[test]
    fn builder_overrides_apply_without_touching_other_fields() {
        let rt = Runtime::builder()
            .debug(true)
            .server_url("http://localhost:9999")
            .build();

        assert!(rt.debug());
        assert_eq!(rt.server_url(), "http://localhost:9999");
        // Untouched fields keep their isolated defaults.
        assert!(!rt.verbose());
        assert!(rt.user_dir().is_some());
    }

    #[test]
    fn default_is_isolated() {
        let rt = Runtime::default();
        assert_ne!(rt.user_dir(), Runtime::default().user_dir());
        assert!(!rt.debug());
    }
}
