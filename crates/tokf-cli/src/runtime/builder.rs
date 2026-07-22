//! Test-only construction of isolated [`Runtime`] values.
//!
//! [`Runtime::isolated`] is the entry point, and [`Runtime::default`] is an
//! alias for it, so the laziest thing a test can write is also the safe thing:
//! there is no constructor that quietly resolves to the developer's real
//! config directory, keychain or `tracking.db`.
//!
//! Each isolated runtime owns a fresh [`tempfile::TempDir`] and a keyring
//! service name unique to that instance, so any number of them can be alive
//! concurrently without interacting.
//!
//! **Note on subprocesses.** A `Runtime` is an in-process value; it does not
//! propagate to child processes. Shims re-exec `tokf -c`, and integration
//! tests spawn the binary, so those paths still travel through `TOKF_HOME` /
//! `TOKF_DB_PATH` environment variables — see `tests/common::TestHome`, which
//! sets them on the child rather than on this process.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Dirs, Flags, Net, OtelEnv, Runtime};

/// Distinguishes the keyring service name of every isolated runtime.
static KEYRING_SEQ: AtomicU64 = AtomicU64::new(0);

impl Default for Runtime {
    /// Equivalent to [`Runtime::isolated`].
    fn default() -> Self {
        Self::isolated()
    }
}

impl Runtime {
    /// Build a runtime whose every path lives inside a fresh temporary
    /// directory, with all flags off and no telemetry configuration.
    ///
    /// The temporary directory is removed when the last clone of the returned
    /// value is dropped.
    ///
    /// # Panics
    ///
    /// Panics if a temporary directory cannot be created, which in a test
    /// environment means the filesystem is unusable.
    #[must_use]
    #[allow(clippy::expect_used)]
    pub fn isolated() -> Self {
        let temp = tempfile::TempDir::new().expect("create temp dir for isolated runtime");
        let seq = KEYRING_SEQ.fetch_add(1, Ordering::Relaxed);
        Self {
            dirs: Dirs::rooted_at(temp.path()),
            flags: Flags::default(),
            net: Net::default(),
            otel: OtelEnv::default(),
            cwd: Some(temp.path().to_path_buf()),
            original_path: None,
            codex_rewrite_mode: None,
            keyring_service: format!("tokf-test-{seq}"),
            temp_root: Some(Arc::new(temp)),
        }
    }

    /// Start from an isolated runtime and override individual fields.
    #[must_use]
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder {
            inner: Self::isolated(),
        }
    }

    /// The root of this runtime's temporary directory, when it has one.
    ///
    /// Useful for seeding files a test needs the runtime to find.
    #[must_use]
    pub fn temp_root(&self) -> Option<&std::path::Path> {
        self.temp_root.as_ref().map(|t| t.path())
    }
}

/// Fluent overrides over an isolated [`Runtime`].
///
/// Anything not overridden keeps its isolated default, so a test states only
/// what it actually cares about.
pub struct RuntimeBuilder {
    inner: Runtime,
}

impl RuntimeBuilder {
    /// Point all user directories at `root` instead of the temporary directory.
    ///
    /// The temporary directory is still owned (and cleaned up) by the runtime;
    /// this only redirects path resolution.
    #[must_use]
    pub fn home(mut self, root: impl Into<std::path::PathBuf>) -> Self {
        let db_path = self.inner.dirs.db_path.take();
        self.inner.dirs = Dirs::rooted_at(root);
        self.inner.dirs.db_path = db_path;
        self
    }

    /// Override the tracking database path.
    #[must_use]
    pub fn db_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.inner.dirs.db_path = Some(path.into());
        self
    }

    /// Clear every resolvable directory, simulating a platform that supplies
    /// none — the `None` branch of the path accessors.
    #[must_use]
    pub fn without_dirs(mut self) -> Self {
        self.inner.dirs = Dirs::default();
        self
    }

    /// Set the working directory used to resolve project-local `.tokf/` paths.
    #[must_use]
    pub fn cwd(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.inner.cwd = Some(dir.into());
        self
    }

    /// Set the `TOKF_DEBUG` flag.
    #[must_use]
    pub const fn debug(mut self, enabled: bool) -> Self {
        self.inner.flags.debug = enabled;
        self
    }

    /// Set the `TOKF_VERBOSE` flag.
    #[must_use]
    pub const fn verbose(mut self, enabled: bool) -> Self {
        self.inner.flags.verbose = enabled;
        self
    }

    /// Set the `TOKF_NO_FILTER` flag.
    #[must_use]
    pub const fn no_filter(mut self, enabled: bool) -> Self {
        self.inner.flags.no_filter = enabled;
        self
    }

    /// Set the `TOKF_SHOW_INDICATOR` override.
    #[must_use]
    pub const fn show_indicator(mut self, value: Option<bool>) -> Self {
        self.inner.flags.show_indicator = value;
        self
    }

    /// Set the tokf server URL.
    #[must_use]
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.inner.net.server_url = url.into();
        self
    }

    /// Set the HTTP request timeout.
    #[must_use]
    pub const fn http_timeout(mut self, timeout: Duration) -> Self {
        self.inner.net.http_timeout = timeout;
        self
    }

    /// Replace the OpenTelemetry environment.
    #[must_use]
    pub fn otel(mut self, otel: OtelEnv) -> Self {
        self.inner.otel = otel;
        self
    }

    /// Set `TOKF_ORIGINAL_PATH`.
    #[must_use]
    pub fn original_path(mut self, path: impl Into<String>) -> Self {
        self.inner.original_path = Some(path.into());
        self
    }

    /// Set `TOKF_CODEX_REWRITE_MODE`.
    #[must_use]
    pub fn codex_rewrite_mode(mut self, mode: impl Into<String>) -> Self {
        self.inner.codex_rewrite_mode = Some(mode.into());
        self
    }

    /// Finish building.
    #[must_use]
    pub fn build(self) -> Runtime {
        self.inner
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn isolated_runtimes_get_distinct_keyring_services() {
        let a = Runtime::isolated();
        let b = Runtime::isolated();
        assert_ne!(a.keyring_service(), b.keyring_service());
        assert!(a.keyring_service().starts_with("tokf-test-"));
    }

    #[test]
    fn the_temp_directory_exists_while_a_clone_is_alive() {
        let clone = {
            let rt = Runtime::isolated();
            let clone = rt.clone();
            drop(rt);
            clone
        };
        assert!(clone.temp_root().unwrap().exists());
    }

    #[test]
    fn the_temp_directory_is_removed_when_the_last_clone_drops() {
        let path = {
            let rt = Runtime::isolated();
            rt.temp_root().unwrap().to_path_buf()
        };
        assert!(!path.exists());
    }

    #[test]
    fn home_redirects_paths_but_keeps_an_explicit_db_path() {
        let rt = Runtime::builder()
            .db_path("/tmp/explicit.db")
            .home("/somewhere/else")
            .build();
        assert_eq!(
            rt.user_dir(),
            Some(std::path::PathBuf::from("/somewhere/else"))
        );
        assert_eq!(
            rt.tracking_db_path(),
            Some(std::path::PathBuf::from("/tmp/explicit.db"))
        );
    }

    #[test]
    fn without_dirs_makes_every_path_unresolvable() {
        let rt = Runtime::builder().without_dirs().build();
        assert_eq!(rt.user_dir(), None);
        assert_eq!(rt.shims_dir(), None);
        assert_eq!(rt.tracking_db_path(), None);
        assert_eq!(rt.global_config_path(), None);
    }

    #[test]
    fn flag_overrides_are_independent() {
        let rt = Runtime::builder()
            .verbose(true)
            .show_indicator(Some(false))
            .build();
        assert!(rt.verbose());
        assert!(!rt.debug());
        assert_eq!(rt.show_indicator(), Some(false));
    }
}
