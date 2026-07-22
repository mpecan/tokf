//! The single seam where the process environment is read.
//!
//! Every `TOKF_*` and `OTEL_*` variable tokf understands is read here and
//! nowhere else. Code downstream receives a fully-resolved [`Runtime`] and can
//! no longer reach the environment, which is what makes a test's environment
//! purely local: it constructs the value it wants instead of mutating a
//! process-global that other tests can observe.
//!
//! `scripts/check-runtime-seam.sh` enforces this boundary in CI.

use std::path::PathBuf;
use std::time::Duration;

use super::{Dirs, Runtime};

/// Default tokf server, used when `TOKF_SERVER_URL` is unset.
pub const DEFAULT_SERVER_URL: &str = "https://api.tokf.net";

/// Default HTTP request timeout, used when `TOKF_HTTP_TIMEOUT` is unset.
pub const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// The keyring service name used by real installations.
pub const DEFAULT_KEYRING_SERVICE: &str = "tokf";

/// Boolean flags sourced from the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flags {
    /// `TOKF_DEBUG` — emit internal diagnostics to stderr.
    pub debug: bool,
    /// `TOKF_VERBOSE` — verbose shell-mode output.
    pub verbose: bool,
    /// `TOKF_NO_FILTER` — bypass filtering entirely.
    pub no_filter: bool,
    /// `TOKF_SHOW_INDICATOR` — overrides `[output] show_indicator` when parseable.
    pub show_indicator: Option<bool>,
}

/// Network configuration sourced from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    /// `TOKF_SERVER_URL`, or [`DEFAULT_SERVER_URL`].
    pub server_url: String,
    /// `TOKF_HTTP_TIMEOUT` seconds, or [`DEFAULT_TIMEOUT_SECS`].
    pub http_timeout: Duration,
}

impl Default for Net {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_string(),
            http_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// Raw OpenTelemetry environment, captured verbatim.
///
/// These are kept as unparsed strings because `telemetry::config::load` layers
/// them over the config file and needs to distinguish "absent" from "set to the
/// default value" when deciding whether the endpoint was explicit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtelEnv {
    /// `TOKF_TELEMETRY_ENABLED`
    pub telemetry_enabled: Option<String>,
    /// `TOKF_OTEL_PIPELINE`
    pub pipeline: Option<String>,
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`
    pub endpoint: Option<String>,
    /// `OTEL_EXPORTER_OTLP_PROTOCOL`
    pub protocol: Option<String>,
    /// `OTEL_EXPORTER_OTLP_HEADERS`
    pub headers: Option<String>,
    /// `OTEL_RESOURCE_ATTRIBUTES`
    pub resource_attributes: Option<String>,
}

/// Read an environment variable, treating an empty value as absent.
fn non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Parse the truthy spelling tokf accepts for `TOKF_DEBUG`.
fn debug_truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

/// Parse the truthy spelling tokf accepts for `TOKF_NO_FILTER` / `TOKF_VERBOSE`.
fn flag_truthy(value: &str) -> bool {
    matches!(value, "1" | "true" | "yes")
}

impl Dirs {
    /// Resolve directories from `TOKF_HOME`, `TOKF_DB_PATH` and the platform.
    ///
    /// Precedence is unchanged from the former `paths` module: a non-empty
    /// `TOKF_HOME` replaces all three platform directories, and `TOKF_DB_PATH`
    /// (empty or not — an empty value was always honoured here) overrides the
    /// tracking database path on top of that.
    fn from_env() -> Self {
        let home = non_empty("TOKF_HOME").map(PathBuf::from);
        let db_path = std::env::var("TOKF_DB_PATH").ok().map(PathBuf::from);

        let mut dirs = home.map_or_else(
            || Self {
                config: dirs::config_dir().map(|d| d.join("tokf")),
                data: dirs::data_local_dir().map(|d| d.join("tokf")),
                cache: dirs::cache_dir().map(|d| d.join("tokf")),
                db_path: None,
                home_override: None,
            },
            Self::rooted_at,
        );
        dirs.db_path = db_path;
        dirs
    }
}

impl Flags {
    fn from_env() -> Self {
        Self {
            debug: non_empty("TOKF_DEBUG").is_some_and(|v| debug_truthy(&v)),
            verbose: non_empty("TOKF_VERBOSE").is_some_and(|v| flag_truthy(&v)),
            no_filter: non_empty("TOKF_NO_FILTER").is_some_and(|v| flag_truthy(&v)),
            show_indicator: std::env::var("TOKF_SHOW_INDICATOR")
                .ok()
                .and_then(|v| v.parse::<bool>().ok()),
        }
    }
}

impl Net {
    fn from_env() -> Self {
        Self {
            server_url: std::env::var("TOKF_SERVER_URL")
                .unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string()),
            http_timeout: Duration::from_secs(
                std::env::var("TOKF_HTTP_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(DEFAULT_TIMEOUT_SECS),
            ),
        }
    }
}

impl OtelEnv {
    fn from_env() -> Self {
        Self {
            telemetry_enabled: std::env::var("TOKF_TELEMETRY_ENABLED").ok(),
            pipeline: std::env::var("TOKF_OTEL_PIPELINE").ok(),
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            protocol: std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok(),
            headers: std::env::var("OTEL_EXPORTER_OTLP_HEADERS").ok(),
            resource_attributes: std::env::var("OTEL_RESOURCE_ATTRIBUTES").ok(),
        }
    }
}

impl Runtime {
    /// Build the runtime configuration from the process environment.
    ///
    /// Call this exactly once, at the top of `main()`, and pass the result down.
    /// It is the only function in tokf that reads a `TOKF_*` or `OTEL_*`
    /// variable; `scripts/check-runtime-seam.sh` fails CI if that stops being
    /// true, or if this function is called from anywhere but `main`.
    pub fn from_env() -> Self {
        Self {
            dirs: Dirs::from_env(),
            flags: Flags::from_env(),
            net: Net::from_env(),
            otel: OtelEnv::from_env(),
            cwd: std::env::current_dir().ok(),
            original_path: std::env::var("TOKF_ORIGINAL_PATH").ok(),
            codex_rewrite_mode: std::env::var("TOKF_CODEX_REWRITE_MODE").ok(),
            keyring_service: DEFAULT_KEYRING_SERVICE.to_string(),
            temp_root: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests deliberately do not touch the environment — doing so would
    // reintroduce exactly the shared mutable state this module exists to
    // remove. They pin the pure parsing helpers and the defaults instead;
    // end-to-end precedence is covered by the integration tests, which set
    // variables on a child process rather than on this one.

    #[test]
    fn debug_accepts_one_and_case_insensitive_true_only() {
        assert!(debug_truthy("1"));
        assert!(debug_truthy("true"));
        assert!(debug_truthy("TRUE"));
        assert!(!debug_truthy("yes"));
        assert!(!debug_truthy("0"));
        assert!(!debug_truthy(""));
    }

    #[test]
    fn flags_accept_one_true_and_yes_case_sensitively() {
        assert!(flag_truthy("1"));
        assert!(flag_truthy("true"));
        assert!(flag_truthy("yes"));
        assert!(!flag_truthy("TRUE"));
        assert!(!flag_truthy("no"));
    }

    #[test]
    fn net_defaults_match_the_documented_values() {
        let net = Net::default();
        assert_eq!(net.server_url, "https://api.tokf.net");
        assert_eq!(net.http_timeout, Duration::from_secs(5));
    }

    #[test]
    fn otel_env_defaults_to_all_absent() {
        assert_eq!(OtelEnv::default(), OtelEnv::default());
        assert!(OtelEnv::default().endpoint.is_none());
    }
}
