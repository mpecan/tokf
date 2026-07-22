//! Shared harness for the CLI integration tests.
//!
//! Integration tests spawn the real `tokf` binary, so they cannot use
//! `Runtime::isolated()` — a `Runtime` is an in-process value and does not
//! cross a process boundary. Isolation has to travel as environment variables
//! instead, which is what this module exists to make automatic.
//!
//! Two things matter equally:
//!
//! - **Setting** `TOKF_HOME` and `TOKF_DB_PATH` into a fresh temporary
//!   directory, so a test never reads or writes the developer's real config
//!   directory, cache, or `tracking.db`.
//! - **Clearing** every other `TOKF_*` and `OTEL_*` variable that happens to
//!   be set in the developer's shell. Without this, `cargo test` on a machine
//!   with (say) `TOKF_DEBUG=1` exported produces different results from a
//!   clean CI container.
//!
//! Use [`tokf`] for the common case. Use [`TestHome`] when the test needs to
//! inspect or seed the home directory, or run several commands against the
//! same one.
//!
//! `scripts/check-runtime-seam.sh` fails CI if a test file reaches for
//! `CARGO_BIN_EXE_tokf` directly instead of going through here.

// Each integration-test binary compiles this module separately and uses a
// different subset of it.
#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Environment variables the `tokf` runtime reads. All of them are cleared
/// before the two isolating ones are set, so an exported value in the
/// developer's shell cannot leak into a test.
const RUNTIME_ENV: &[&str] = &[
    "TOKF_HOME",
    "TOKF_DB_PATH",
    "TOKF_DEBUG",
    "TOKF_VERBOSE",
    "TOKF_NO_FILTER",
    "TOKF_SHOW_INDICATOR",
    "TOKF_SERVER_URL",
    "TOKF_HTTP_TIMEOUT",
    "TOKF_ORIGINAL_PATH",
    "TOKF_CODEX_REWRITE_MODE",
    "TOKF_TELEMETRY_ENABLED",
    "TOKF_OTEL_PIPELINE",
    // Read via clap `#[arg(env = ...)]` in cli_args.rs rather than through
    // Runtime — TOKF_PRESERVE_COLOR in particular changes stdout formatting
    // and fails colour-sensitive assertions when exported.
    "TOKF_PRESERVE_COLOR",
    "TOKF_REGISTRY_URL",
    "TOKF_SERVICE_TOKEN",
    // Runtime-owned, but still cleared here: a spawned binary would otherwise
    // append a diagnostic record for every hook invocation to a path of the
    // developer's choosing, i.e. outside the test's temp home.
    "TOKF_HOOK_LOG",
    "OTEL_EXPORTER_OTLP_ENDPOINT",
    "OTEL_EXPORTER_OTLP_PROTOCOL",
    "OTEL_EXPORTER_OTLP_HEADERS",
    "OTEL_RESOURCE_ATTRIBUTES",
];

/// Point `cmd` at `home` for all tokf state, clearing every other runtime
/// variable first.
///
/// Apply this to **non-tokf** spawns too — `git`, `make`, `just`. A developer
/// machine with `tokf hook install` run has tokf shims on `PATH`, so those
/// tools re-enter tokf through a shim. Without this the shim'd process finds
/// no `TOKF_HOME`, falls back to the platform default, and records the test's
/// command into the developer's real `tracking.db`.
pub fn isolate_env(cmd: &mut Command, home: &Path) {
    for key in RUNTIME_ENV {
        cmd.env_remove(key);
    }
    cmd.env("TOKF_HOME", home);
    cmd.env("TOKF_DB_PATH", home.join("tracking.db"));
}

/// A `tokf` command whose user directory and tracking database point into
/// `home`, with every other runtime variable cleared.
pub fn isolated_command(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    isolate_env(&mut cmd, home);
    cmd
}

/// Spawn an external tool (`git`, `make`, …) in `dir`, with tokf state
/// isolated to that same directory, so any tokf shim it re-enters cannot
/// reach the real config.
pub fn isolated_tool(program: &str, dir: &Path) -> Command {
    let mut cmd = Command::new(program);
    isolate_env(&mut cmd, dir);
    cmd.current_dir(dir);
    cmd
}

/// A throwaway tokf home directory, removed when the value is dropped.
///
/// Hold one when a test needs to seed config files, inspect what the binary
/// wrote, or run several commands against the same home.
pub struct TestHome {
    dir: TempDir,
}

impl TestHome {
    pub fn new() -> Self {
        Self {
            dir: TempDir::new().expect("create temp home"),
        }
    }

    /// The home directory itself (`TOKF_HOME`).
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// The tracking database inside this home (`TOKF_DB_PATH`).
    pub fn db_path(&self) -> PathBuf {
        self.dir.path().join("tracking.db")
    }

    /// A `tokf` command isolated to this home.
    pub fn cmd(&self) -> Command {
        isolated_command(self.path())
    }
}

impl Default for TestHome {
    fn default() -> Self {
        Self::new()
    }
}

/// An isolated `tokf` command that owns its own throwaway home.
///
/// Derefs to [`Command`], so it is a drop-in for the per-file `fn tokf()`
/// helpers this replaces: `tokf().args(["ls"]).output()` still works. The
/// temporary directory lives until the end of the statement, which covers the
/// whole builder chain up to and including `output()` / `status()`.
///
/// **Do not use with `spawn()`.** That returns while the child is still
/// running, and the temporary home is deleted at the end of the statement —
/// out from under it. Bind a [`TestHome`] and use [`TestHome::cmd`] instead,
/// so the directory outlives the child.
pub struct TokfCommand {
    cmd: Command,
    home: TestHome,
}

impl TokfCommand {
    /// The home directory backing this command.
    pub fn home(&self) -> &Path {
        self.home.path()
    }
}

impl std::ops::Deref for TokfCommand {
    type Target = Command;

    fn deref(&self) -> &Command {
        &self.cmd
    }
}

impl std::ops::DerefMut for TokfCommand {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.cmd
    }
}

/// An isolated `tokf` command backed by a fresh temporary home.
pub fn tokf() -> TokfCommand {
    let home = TestHome::new();
    let cmd = isolated_command(home.path());
    TokfCommand { cmd, home }
}
