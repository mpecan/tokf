//! The single seam where the process environment is read.
//!
//! Every `TOKF_*` and `OTEL_*` variable that feeds a [`Runtime`] is read here
//! and nowhere else. Code downstream receives a fully-resolved value and can
//! no longer reach the environment, which is what makes a test's environment
//! purely local: it constructs the value it wants instead of mutating a
//! process-global that other tests can observe.
//!
//! There is a second, deliberate seam: clap reads `TOKF_PRESERVE_COLOR`,
//! `TOKF_REGISTRY_URL` and `TOKF_SERVICE_TOKEN` via `#[arg(env = ...)]` in
//! `cli_args.rs`. Those are flag defaults rather than runtime configuration,
//! so they stay with their commands — but they must still be listed in
//! `RUNTIME_ENV` in `tests/common/mod.rs` so the integration harness clears
//! them.
//!
//! `scripts/check-runtime-seam.sh` enforces both halves in CI.

use std::path::PathBuf;
use std::time::Duration;

use super::{Dirs, Runtime};

/// Default tokf server, used when `TOKF_SERVER_URL` is unset.
pub const DEFAULT_SERVER_URL: &str = "https://api.tokf.net";

/// Default HTTP request timeout, used when `TOKF_HTTP_TIMEOUT` is unset.
pub const DEFAULT_TIMEOUT_SECS: u64 = 5;

/// The keyring service name used by real installations.
pub(super) const DEFAULT_KEYRING_SERVICE: &str = "tokf";

/// Boolean flags sourced from the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Flags {
    /// `TOKF_DEBUG` — emit internal diagnostics to stderr.
    pub(super) debug: bool,
    /// `TOKF_VERBOSE` — verbose shell-mode output.
    pub(super) verbose: bool,
    /// `TOKF_NO_FILTER` — bypass filtering entirely.
    pub(super) no_filter: bool,
    /// `TOKF_SHOW_INDICATOR` — overrides `[output] show_indicator` when parseable.
    pub(super) show_indicator: Option<bool>,
}

/// Network configuration sourced from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Net {
    /// `TOKF_SERVER_URL`, or [`DEFAULT_SERVER_URL`].
    pub(super) server_url: String,
    /// `TOKF_HTTP_TIMEOUT` seconds, or [`DEFAULT_TIMEOUT_SECS`].
    pub(super) http_timeout: Duration,
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

/// How a variable is looked up. Real runs pass `std::env::var`; tests pass a
/// map, which is what makes the precedence rules below testable without
/// mutating the process environment.
type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The production lookup.
fn env_lookup(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Read a variable, treating an empty value as absent.
fn non_empty(get: Lookup<'_>, key: &str) -> Option<String> {
    get(key).filter(|s| !s.is_empty())
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
    ///
    /// Note the deliberate asymmetry: an **empty** `TOKF_HOME` is ignored and
    /// falls through to the platform directories, while an empty
    /// `TOKF_DB_PATH` is taken at face value.
    fn from_lookup(get: Lookup<'_>) -> Self {
        let home = non_empty(get, "TOKF_HOME").map(PathBuf::from);
        let db_path = get("TOKF_DB_PATH").map(PathBuf::from);

        let mut dirs = home.map_or_else(Self::platform, Self::rooted_at);
        dirs.db_path = db_path;
        dirs
    }

    /// The platform-native directories, each with tokf's own subdirectory.
    fn platform() -> Self {
        Self {
            config: dirs::config_dir().map(|d| d.join("tokf")),
            data: dirs::data_local_dir().map(|d| d.join("tokf")),
            cache: dirs::cache_dir().map(|d| d.join("tokf")),
            db_path: None,
            home_override: None,
        }
    }
}

impl Flags {
    fn from_lookup(get: Lookup<'_>) -> Self {
        Self {
            debug: non_empty(get, "TOKF_DEBUG").is_some_and(|v| debug_truthy(&v)),
            verbose: non_empty(get, "TOKF_VERBOSE").is_some_and(|v| flag_truthy(&v)),
            no_filter: non_empty(get, "TOKF_NO_FILTER").is_some_and(|v| flag_truthy(&v)),
            show_indicator: get("TOKF_SHOW_INDICATOR").and_then(|v| v.parse::<bool>().ok()),
        }
    }
}

impl Net {
    fn from_lookup(get: Lookup<'_>) -> Self {
        Self {
            server_url: get("TOKF_SERVER_URL").unwrap_or_else(|| DEFAULT_SERVER_URL.to_string()),
            http_timeout: Duration::from_secs(
                get("TOKF_HTTP_TIMEOUT")
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(DEFAULT_TIMEOUT_SECS),
            ),
        }
    }
}

impl OtelEnv {
    fn from_lookup(get: Lookup<'_>) -> Self {
        Self {
            telemetry_enabled: get("TOKF_TELEMETRY_ENABLED"),
            pipeline: get("TOKF_OTEL_PIPELINE"),
            endpoint: get("OTEL_EXPORTER_OTLP_ENDPOINT"),
            protocol: get("OTEL_EXPORTER_OTLP_PROTOCOL"),
            headers: get("OTEL_EXPORTER_OTLP_HEADERS"),
            resource_attributes: get("OTEL_RESOURCE_ATTRIBUTES"),
        }
    }
}

impl Runtime {
    /// Build the runtime configuration from the process environment.
    ///
    /// Call this exactly once, at the top of `main()`, and pass the result down.
    /// `scripts/check-runtime-seam.sh` fails CI if it is called anywhere else.
    pub fn from_env() -> Self {
        Self::from_lookup(&env_lookup, std::env::current_dir().ok())
    }

    /// The whole of [`Runtime::from_env`] except the two process-level reads,
    /// so every precedence rule above is reachable from a test.
    fn from_lookup(get: Lookup<'_>, cwd: Option<PathBuf>) -> Self {
        Self {
            dirs: Dirs::from_lookup(get),
            flags: Flags::from_lookup(get),
            net: Net::from_lookup(get),
            otel: OtelEnv::from_lookup(get),
            cwd,
            original_path: get("TOKF_ORIGINAL_PATH"),
            codex_rewrite_mode: get("TOKF_CODEX_REWRITE_MODE"),
            hook_log: non_empty(get, "TOKF_HOOK_LOG").map(PathBuf::from),
            keyring_service: DEFAULT_KEYRING_SERVICE.to_string(),
            temp_root: None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Build a lookup over a fixed map, so precedence and parsing are testable
    /// without mutating the process environment — which is the shared mutable
    /// state this module exists to remove.
    fn lookup(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn dirs_from(pairs: &[(&str, &str)]) -> Dirs {
        let map = lookup(pairs);
        Dirs::from_lookup(&|k| map.get(k).cloned())
    }

    fn flags_from(pairs: &[(&str, &str)]) -> Flags {
        let map = lookup(pairs);
        Flags::from_lookup(&|k| map.get(k).cloned())
    }

    fn net_from(pairs: &[(&str, &str)]) -> Net {
        let map = lookup(pairs);
        Net::from_lookup(&|k| map.get(k).cloned())
    }

    fn runtime_from(pairs: &[(&str, &str)]) -> Runtime {
        let map = lookup(pairs);
        Runtime::from_lookup(&|k| map.get(k).cloned(), None)
    }

    // -- parsing ----------------------------------------------------------

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

    // -- TOKF_HOME --------------------------------------------------------

    #[test]
    fn tokf_home_replaces_all_three_platform_directories() {
        let dirs = dirs_from(&[("TOKF_HOME", "/custom/home")]);
        assert_eq!(dirs.user_dir(), Some(PathBuf::from("/custom/home")));
        assert_eq!(dirs.user_data_dir(), Some(PathBuf::from("/custom/home")));
        assert_eq!(dirs.user_cache_dir(), Some(PathBuf::from("/custom/home")));
    }

    /// An empty `TOKF_HOME` must be ignored, not treated as the root path.
    #[test]
    fn empty_tokf_home_falls_back_to_the_platform_directories() {
        let dirs = dirs_from(&[("TOKF_HOME", "")]);
        assert_eq!(dirs.user_dir(), dirs::config_dir().map(|d| d.join("tokf")));
        assert_eq!(dirs.home_override(), None);
    }

    #[test]
    fn unset_tokf_home_uses_the_platform_directories() {
        let dirs = dirs_from(&[]);
        assert_eq!(dirs.user_dir(), dirs::config_dir().map(|d| d.join("tokf")));
        assert_eq!(
            dirs.user_data_dir(),
            dirs::data_local_dir().map(|d| d.join("tokf"))
        );
        assert_eq!(
            dirs.user_cache_dir(),
            dirs::cache_dir().map(|d| d.join("tokf"))
        );
    }

    /// The three platform directories must come from three *different* `dirs`
    /// functions, each with tokf's own subdirectory — a swap or a typo'd join
    /// would otherwise go unnoticed.
    #[test]
    fn platform_directories_are_distinct_and_namespaced() {
        let dirs = Dirs::platform();
        for d in [dirs.user_dir(), dirs.user_data_dir(), dirs.user_cache_dir()]
            .into_iter()
            .flatten()
        {
            assert_eq!(
                d.file_name().and_then(|n| n.to_str()),
                Some("tokf"),
                "every platform dir must end in the tokf namespace: {}",
                d.display()
            );
        }
        // On every platform tokf supports, config and cache are distinct roots.
        if let (Some(config), Some(cache)) = (dirs.user_dir(), dirs.user_cache_dir()) {
            assert_ne!(config, cache, "config and cache must not be the same dir");
        }
    }

    // -- TOKF_DB_PATH -----------------------------------------------------

    #[test]
    fn db_path_overrides_tokf_home() {
        let dirs = dirs_from(&[
            ("TOKF_HOME", "/custom/home"),
            ("TOKF_DB_PATH", "/db/at.sqlite"),
        ]);
        assert_eq!(
            dirs.tracking_db_path(),
            Some(PathBuf::from("/db/at.sqlite"))
        );
        // ...without disturbing the other directories.
        assert_eq!(dirs.user_dir(), Some(PathBuf::from("/custom/home")));
    }

    #[test]
    fn tokf_home_supplies_the_db_path_when_no_override_is_set() {
        let dirs = dirs_from(&[("TOKF_HOME", "/custom/home")]);
        assert_eq!(
            dirs.tracking_db_path(),
            Some(PathBuf::from("/custom/home/tracking.db"))
        );
    }

    /// Unlike `TOKF_HOME`, an empty `TOKF_DB_PATH` is honoured verbatim. This
    /// asymmetry is inherited from the module this replaced; pin it so a
    /// well-meaning "tidy-up" cannot silently change behaviour.
    #[test]
    fn empty_db_path_is_honoured_verbatim() {
        let dirs = dirs_from(&[("TOKF_DB_PATH", "")]);
        assert_eq!(dirs.tracking_db_path(), Some(PathBuf::new()));
    }

    // -- flags ------------------------------------------------------------

    #[test]
    fn flags_are_off_when_unset() {
        let flags = flags_from(&[]);
        assert!(!flags.debug);
        assert!(!flags.verbose);
        assert!(!flags.no_filter);
        assert_eq!(flags.show_indicator, None);
    }

    #[test]
    fn each_flag_reads_its_own_variable() {
        assert!(flags_from(&[("TOKF_DEBUG", "1")]).debug);
        assert!(flags_from(&[("TOKF_VERBOSE", "yes")]).verbose);
        assert!(flags_from(&[("TOKF_NO_FILTER", "true")]).no_filter);
        assert_eq!(
            flags_from(&[("TOKF_SHOW_INDICATOR", "false")]).show_indicator,
            Some(false)
        );
        // An unparseable value is ignored rather than treated as `true`.
        assert_eq!(
            flags_from(&[("TOKF_SHOW_INDICATOR", "banana")]).show_indicator,
            None
        );
    }

    // -- network ----------------------------------------------------------

    #[test]
    fn net_defaults_match_the_documented_values() {
        let net = net_from(&[]);
        assert_eq!(net.server_url, DEFAULT_SERVER_URL);
        assert_eq!(net.http_timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    #[test]
    fn net_reads_its_variables() {
        let net = net_from(&[
            ("TOKF_SERVER_URL", "https://staging.example.com"),
            ("TOKF_HTTP_TIMEOUT", "42"),
        ]);
        assert_eq!(net.server_url, "https://staging.example.com");
        assert_eq!(net.http_timeout, Duration::from_secs(42));
    }

    #[test]
    fn an_unparseable_timeout_falls_back_to_the_default() {
        let net = net_from(&[("TOKF_HTTP_TIMEOUT", "soon")]);
        assert_eq!(net.http_timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    // -- whole runtime ----------------------------------------------------

    #[test]
    fn otel_variables_are_captured_verbatim() {
        let rt = runtime_from(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            ("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc"),
            ("OTEL_EXPORTER_OTLP_HEADERS", "k=v"),
            ("OTEL_RESOURCE_ATTRIBUTES", "service.name=x"),
            ("TOKF_TELEMETRY_ENABLED", "true"),
            ("TOKF_OTEL_PIPELINE", "ci"),
        ]);
        let otel = rt.otel();
        assert_eq!(otel.endpoint.as_deref(), Some("http://collector:4318"));
        assert_eq!(otel.protocol.as_deref(), Some("grpc"));
        assert_eq!(otel.headers.as_deref(), Some("k=v"));
        assert_eq!(otel.resource_attributes.as_deref(), Some("service.name=x"));
        assert_eq!(otel.telemetry_enabled.as_deref(), Some("true"));
        assert_eq!(otel.pipeline.as_deref(), Some("ci"));
    }

    #[test]
    fn process_level_variables_reach_the_runtime() {
        let rt = runtime_from(&[
            ("TOKF_ORIGINAL_PATH", "/usr/bin:/bin"),
            ("TOKF_CODEX_REWRITE_MODE", "updated-input"),
        ]);
        assert_eq!(rt.original_path(), Some("/usr/bin:/bin"));
        assert_eq!(rt.codex_rewrite_mode(), Some("updated-input"));
    }

    #[test]
    fn a_runtime_from_the_environment_uses_the_real_keyring_service() {
        let rt = runtime_from(&[]);
        assert_eq!(rt.keyring_service(), DEFAULT_KEYRING_SERVICE);
        // ...and owns no temp dir, unlike an isolated one.
        assert_eq!(rt.temp_root(), None);
    }

    #[test]
    fn an_empty_environment_yields_the_documented_defaults() {
        let rt = runtime_from(&[]);
        assert!(!rt.debug());
        assert_eq!(rt.server_url(), DEFAULT_SERVER_URL);
        assert_eq!(rt.otel(), &OtelEnv::default());
        assert_eq!(rt.original_path(), None);
    }
}
