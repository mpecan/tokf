use tokf::config;
use tokf::config::types::FilterConfig;
use tokf::runner;
use tokf::tracking;

/// Result of filter resolution, including any deferred output-pattern variants.
pub struct FilterMatch {
    pub config: FilterConfig,
    /// Canonical hash of the Phase A resolved config.
    pub hash: String,
    pub words_consumed: usize,
    pub output_variants: Vec<config::variant::DeferredVariant>,
    /// The full resolved filter list, kept for Phase B output-pattern resolution.
    pub resolved_filters: Vec<config::ResolvedFilter>,
}

/// Discover all filters using the standard search dirs + cache.
pub fn discover_filters(no_cache: bool) -> anyhow::Result<Vec<config::ResolvedFilter>> {
    let search_dirs = config::default_search_dirs();
    if no_cache {
        config::discover_all_filters(&search_dirs)
    } else {
        config::cache::discover_with_cache(&search_dirs)
    }
}

/// Find the first filter that matches `command_args` using the discovery model.
pub fn find_filter(
    command_args: &[String],
    verbose: bool,
    no_cache: bool,
) -> anyhow::Result<Option<FilterMatch>> {
    let resolved = discover_filters(no_cache)?;
    let words: Vec<&str> = command_args.iter().map(String::as_str).collect();
    let cwd = std::env::current_dir().unwrap_or_default();

    for filter in &resolved {
        if let Some(consumed) = filter.matches(&words) {
            if verbose {
                eprintln!(
                    "[tokf] matched {} (command: \"{}\") in {}",
                    filter.relative_path.display(),
                    filter.config.command.first(),
                    filter
                        .source_path
                        .parent()
                        .map_or("?", |p| p.to_str().unwrap_or("?")),
                );
            }

            // Phase A: resolve file-based variants
            if filter.config.variant.is_empty() {
                return Ok(Some(FilterMatch {
                    config: filter.config.clone(),
                    hash: filter.hash.clone(),
                    words_consumed: consumed,
                    output_variants: vec![],
                    resolved_filters: resolved,
                }));
            }

            let resolution =
                config::variant::resolve_variants(&filter.config, &resolved, &cwd, verbose);
            let hash = tokf_common::hash::canonical_hash(&resolution.config)
                .unwrap_or_else(|_| filter.hash.clone());
            return Ok(Some(FilterMatch {
                config: resolution.config,
                hash,
                words_consumed: consumed,
                output_variants: resolution.output_variants,
                resolved_filters: resolved,
            }));
        }
    }

    if verbose {
        eprintln!(
            "[tokf] no filter found for '{}', passing through",
            words.join(" ")
        );
    }
    Ok(None)
}

/// Resolve Phase B output-pattern variants using the already-discovered filter list.
///
/// Returns `(FilterConfig, hash)` where `hash` is recomputed from the final config
/// when an output-pattern variant fires, or the Phase A hash otherwise.
pub fn resolve_phase_b(
    filter_match: FilterMatch,
    output: &str,
    verbose: bool,
) -> (FilterConfig, String) {
    if filter_match.output_variants.is_empty() {
        return (filter_match.config, filter_match.hash);
    }
    let original_hash = filter_match.hash.clone();
    let cfg = config::variant::resolve_output_variants(
        &filter_match.output_variants,
        output,
        &filter_match.resolved_filters,
        verbose,
    )
    .unwrap_or(filter_match.config);
    let hash = tokf_common::hash::canonical_hash(&cfg).unwrap_or(original_hash);
    (cfg, hash)
}

/// Build environment variable overrides for `inject_path` mode.
///
/// When the filter has `inject_path = true` and shims exist on disk,
/// returns env entries that prepend the shims dir to `PATH`, save the
/// original `PATH` as `TOKF_ORIGINAL_PATH`, and set `SHELL=tokf`.
///
/// **Note:** `ShimsConfig` is loaded with `project_root = None` (global config only).
/// This is intentional — `build_inject_env` runs in the hot path after every
/// filtered command, so we skip the filesystem walk to locate `.tokf/config.toml`
/// for performance. Users who need to disable shims can set `shims.enabled = false`
/// in their global config.
fn build_inject_env(filter_cfg: Option<&FilterConfig>) -> Vec<(String, String)> {
    let Some(cfg) = filter_cfg else {
        return vec![];
    };
    if !cfg.inject_path {
        return vec![];
    }
    let shims_config = tokf::history::ShimsConfig::load(None);
    if !shims_config.enabled {
        return vec![];
    }
    let Some(shims) = tokf::paths::shims_dir() else {
        return vec![];
    };
    if !shims.exists() {
        return vec![];
    }
    // Use TOKF_ORIGINAL_PATH if already set (nested tokf invocation)
    // to avoid stacking shims in PATH repeatedly.
    let original_path = std::env::var("TOKF_ORIGINAL_PATH")
        .or_else(|_| std::env::var("PATH"))
        .unwrap_or_default();
    let new_path = format!("{}:{}", shims.display(), original_path);
    let tokf_exe = std::env::current_exe()
        .unwrap_or_else(|_| "tokf".into())
        .to_string_lossy()
        .into_owned();

    vec![
        ("PATH".to_string(), new_path),
        ("TOKF_ORIGINAL_PATH".to_string(), original_path),
        ("SHELL".to_string(), tokf_exe),
    ]
}

pub fn run_command(
    filter_cfg: Option<&FilterConfig>,
    words_consumed: usize,
    command_args: &[String],
    remaining_args: &[String],
) -> anyhow::Result<runner::CommandResult> {
    let env_overrides = build_inject_env(filter_cfg);
    let env_refs: Vec<(&str, &str)> = env_overrides
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    if let Some(cfg) = filter_cfg
        && let Some(run_cmd) = &cfg.run
    {
        runner::execute_shell_with_env(run_cmd, remaining_args, &env_refs)
    } else if words_consumed > 0 {
        let cmd_str = command_args[..words_consumed].join(" ");
        runner::execute_with_env(&cmd_str, remaining_args, &env_refs)
    } else {
        runner::execute_with_env(&command_args[0], remaining_args, &env_refs)
    }
}

/// Attempt a background auto-sync if the pending event count exceeds the configured threshold.
///
/// All checks are cheap (no network I/O) — only spawns a detached `tokf sync` process
/// when all preconditions are met.
///
/// **Note:** `upload_usage_stats` is read from the global config only (project root = `None`).
/// This is intentional — `try_auto_sync` runs in the hot path after every filtered command,
/// so we skip the filesystem walk to locate `.tokf/config.toml` for performance. Users who
/// need per-project overrides can set `upload_usage_stats` in their global config instead.
pub fn try_auto_sync() {
    use std::process::{Command, Stdio};
    use tokf::auth::credentials;
    use tokf::history::SyncConfig;
    use tokf::remote::machine;

    // Pass None for project dir: auto-sync runs in the hot path after every command,
    // so we only check the global config to avoid a filesystem scan for .tokf/config.toml.
    let config = SyncConfig::load(None);
    if config.auto_sync_threshold == 0 {
        return;
    }

    if !config.upload_usage_stats.unwrap_or(false) {
        return; // None → never asked, Some(false) → opted out
    }

    if credentials::load().is_none() {
        return;
    }
    if machine::load().is_none() {
        return;
    }

    let Some(db_path) = tracking::db_path() else {
        return;
    };
    let Ok(conn) = tracking::open_db(&db_path) else {
        return;
    };
    let Ok(pending) = tracking::get_pending_count(&conn) else {
        return;
    };

    if pending < i64::from(config.auto_sync_threshold) {
        return;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| "tokf".into());
    match Command::new(exe)
        .args(["sync"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(e) => {
            if tokf::paths::debug_enabled() {
                eprintln!("[tokf] auto-sync spawn failed: {e}");
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_run(
    command_args: &[String],
    filter_name: Option<&str>,
    filter_hash: Option<&str>,
    input_bytes: usize,
    output_bytes: usize,
    raw_bytes: usize,
    filter_time_ms: u128,
    exit_code: i32,
    pipe_override: bool,
) {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] tracking: cannot determine DB path");
        return;
    };
    let conn = match tracking::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] tracking error (db open): {e:#}");
            eprintln!(
                "[tokf] hint: set TOKF_DB_PATH to choose a different DB path, \
                 or TOKF_HOME to relocate all tokf data"
            );
            return;
        }
    };
    let command = command_args.join(" ");
    let event = tracking::build_event(
        &command,
        filter_name,
        filter_hash,
        input_bytes,
        output_bytes,
        raw_bytes,
        filter_time_ms,
        exit_code,
        pipe_override,
    );
    if let Err(e) = tracking::record_event(&conn, &event) {
        eprintln!(
            "[tokf] tracking error (record) at {}: {e:#}",
            path.display()
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use tokf::config::types::FilterConfig;

    use super::*;

    fn config_with_inject(inject: bool) -> FilterConfig {
        let toml = format!("command = \"git commit\"\ninject_path = {inject}");
        toml::from_str(&toml).unwrap()
    }

    #[test]
    fn build_inject_env_empty_when_no_config() {
        assert!(build_inject_env(None).is_empty());
    }

    #[test]
    fn build_inject_env_empty_when_disabled() {
        let cfg = config_with_inject(false);
        assert!(build_inject_env(Some(&cfg)).is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn build_inject_env_empty_when_shims_dir_missing() {
        let _guard = tokf::paths::HomeGuard::set("/nonexistent/path/tokf_test");
        let cfg = config_with_inject(true);
        // shims_dir exists in theory but the directory doesn't exist on disk
        assert!(build_inject_env(Some(&cfg)).is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn build_inject_env_returns_three_vars_when_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _guard = tokf::paths::HomeGuard::set(tmp.path());
        let shims = tokf::paths::shims_dir().unwrap();
        std::fs::create_dir_all(&shims).unwrap();

        let cfg = config_with_inject(true);
        let env = build_inject_env(Some(&cfg));

        assert_eq!(env.len(), 3);
        assert_eq!(env[0].0, "PATH");
        assert!(env[0].1.starts_with(&shims.to_string_lossy().to_string()));
        assert_eq!(env[1].0, "TOKF_ORIGINAL_PATH");
        assert_eq!(env[2].0, "SHELL");
    }

    #[test]
    #[serial_test::serial]
    fn build_inject_env_uses_original_path_when_nested() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _guard = tokf::paths::HomeGuard::set(tmp.path());
        let shims = tokf::paths::shims_dir().unwrap();
        std::fs::create_dir_all(&shims).unwrap();

        // Simulate nested invocation: TOKF_ORIGINAL_PATH is already set
        // SAFETY: test runs serially (#[serial]) so no concurrent access.
        unsafe { std::env::set_var("TOKF_ORIGINAL_PATH", "/usr/bin:/bin") };
        let cfg = config_with_inject(true);
        let env = build_inject_env(Some(&cfg));
        unsafe { std::env::remove_var("TOKF_ORIGINAL_PATH") };

        // PATH should be shims:/usr/bin:/bin (not shims:shims:/usr/bin:/bin)
        assert_eq!(env[1].0, "TOKF_ORIGINAL_PATH");
        assert_eq!(env[1].1, "/usr/bin:/bin");
        assert!(
            env[0]
                .1
                .starts_with(&format!("{}:/usr/bin:/bin", shims.display()))
        );
    }
}
