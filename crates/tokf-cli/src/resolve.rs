use tokf::config;
use tokf::config::types::FilterConfig;
use tokf::history::current_project;
use tokf::runner;
use tokf::tracking;

use tokf::runtime::Runtime;

use crate::path_env::prepend_to_path;

/// Result of filter resolution, including any deferred output-pattern variants.
pub struct FilterMatch {
    pub config: FilterConfig,
    /// Canonical hash of the Phase A resolved config.
    pub hash: String,
    pub words_consumed: usize,
    pub matched_command: String,
    pub output_variants: Vec<config::variant::DeferredVariant>,
    /// The full resolved filter list, kept for Phase B output-pattern resolution.
    pub resolved_filters: Vec<config::ResolvedFilter>,
}

/// Discover all filters using the standard search dirs + cache.
pub fn discover_filters(
    rt: &Runtime,
    no_cache: bool,
) -> anyhow::Result<Vec<config::ResolvedFilter>> {
    let search_dirs = config::default_search_dirs(rt);
    if no_cache {
        config::discover_all_filters(&search_dirs)
    } else {
        config::cache::discover_with_cache(rt, &search_dirs)
    }
}

/// Find the first filter that matches `command_args` using the discovery model.
pub fn find_filter(
    rt: &Runtime,
    command_args: &[String],
    verbose: bool,
    no_cache: bool,
) -> anyhow::Result<Option<FilterMatch>> {
    let resolved = discover_filters(rt, no_cache)?;
    let words: Vec<&str> = command_args.iter().map(String::as_str).collect();
    let cwd = rt.cwd_or_empty();
    let wrapper_cfg = tokf::rewrite::load_local_wrapper_config(rt);

    // Match directly, or after stripping a local environment wrapper prefix
    // (e.g. `nix develop -c cargo test` matches the `cargo test` filter). The
    // returned `consumed` spans the full prefix — wrapper words plus the matched
    // pattern — so the whole command is executed and its output filtered.
    if let Some((filter, matched_command, consumed)) =
        config::local_wrapper::match_filters_with_wrapper(&resolved, &words, &wrapper_cfg)
    {
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
                matched_command: matched_command.to_string(),
                output_variants: vec![],
                resolved_filters: resolved,
            }));
        }

        let resolution = config::variant::resolve_variants(&filter.config, &resolved, cwd, verbose);
        let hash = tokf_common::hash::canonical_hash(&resolution.config)
            .unwrap_or_else(|_| filter.hash.clone());
        return Ok(Some(FilterMatch {
            config: resolution.config,
            hash,
            words_consumed: consumed,
            matched_command: matched_command.to_string(),
            output_variants: resolution.output_variants,
            resolved_filters: resolved,
        }));
    }

    if verbose {
        eprintln!(
            "[tokf] no filter found for '{}', passing through",
            words.join(" ")
        );
    }
    Ok(None)
}

/// Resolve Phase A.5 args-pattern variants.
///
/// When an args variant matches, returns an updated `FilterMatch` with the
/// variant's config (and clears `output_variants`, since the parent's
/// deferred variants no longer apply to the delegated filter).
pub fn resolve_args_variants(
    filter_match: FilterMatch,
    remaining_args: &[String],
    verbose: bool,
) -> FilterMatch {
    if filter_match.config.variant.is_empty() || remaining_args.is_empty() {
        return filter_match;
    }
    if let Some(cfg) = config::variant::resolve_args_variants(
        &filter_match.config,
        &filter_match.resolved_filters,
        remaining_args,
        verbose,
    ) {
        let hash =
            tokf_common::hash::canonical_hash(&cfg).unwrap_or_else(|_| filter_match.hash.clone());
        FilterMatch {
            config: cfg,
            hash,
            words_consumed: filter_match.words_consumed,
            matched_command: filter_match.matched_command,
            output_variants: vec![],
            resolved_filters: filter_match.resolved_filters,
        }
    } else {
        filter_match
    }
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
/// original `PATH` as `TOKF_ORIGINAL_PATH`, and set `SHELL` to the tokf
/// executable path.
///
/// **Note:** `ShimsConfig` is loaded with `project_root = None` (global config only).
/// This is intentional — `build_inject_env` runs in the hot path after every
/// filtered command, so we skip the filesystem walk to locate `.tokf/config.toml`
/// for performance. Users who need to disable shims can set `shims.enabled = false`
/// in their global config.
fn build_inject_env(rt: &Runtime, filter_cfg: Option<&FilterConfig>) -> Vec<(String, String)> {
    let Some(cfg) = filter_cfg else {
        return vec![];
    };
    if !cfg.inject_path {
        return vec![];
    }
    let shims_config = tokf::history::ShimsConfig::load(rt, None);
    if !shims_config.enabled {
        return vec![];
    }
    let Some(shims) = rt.shims_dir() else {
        return vec![];
    };
    if !shims.exists() {
        return vec![];
    }
    // Use TOKF_ORIGINAL_PATH if already set (nested tokf invocation)
    // to avoid stacking shims in PATH repeatedly.
    let original_path = rt
        .original_path()
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let new_path = prepend_to_path(&shims, &original_path);
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

fn run_command_with_consumed_prefix(
    run_cmd: &str,
    matched_command: &str,
    command_args: &[String],
    words_consumed: usize,
) -> String {
    let pattern_words = matched_command.split_whitespace().count();
    if words_consumed <= pattern_words || command_args.len() < words_consumed {
        return run_cmd.to_string();
    }

    let trimmed = run_cmd.trim_start();
    let leading_len = run_cmd.len() - trimmed.len();
    let leading = &run_cmd[..leading_len];
    let Some(suffix) = trimmed.strip_prefix(matched_command) else {
        return run_cmd.to_string();
    };
    if !suffix.is_empty() && !suffix.starts_with(char::is_whitespace) {
        return run_cmd.to_string();
    }

    let mut prefix = command_args[0].clone();
    let quoted_args = crate::shell::quote_argv(&command_args[1..words_consumed]);
    if !quoted_args.is_empty() {
        prefix.push(' ');
        prefix.push_str(&quoted_args);
    }
    format!("{leading}{prefix}{suffix}")
}

/// A resolved command, ready to execute.
#[derive(Clone, Copy)]
pub struct ResolvedCommand<'a> {
    pub filter_cfg: Option<&'a FilterConfig>,
    pub words_consumed: usize,
    pub matched_command: Option<&'a str>,
    pub command_args: &'a [String],
    pub remaining_args: &'a [String],
    pub verbose: bool,
}

/// Execute the resolved command.
///
/// Returns the command result together with the command tokf actually ran when
/// a filter's `run` override replaced what the user typed, and `None` when the
/// user's command was run verbatim. Callers must record that string: without it
/// history entries, `tokf raw` and savings figures would all be labelled with a
/// command that never produced the captured output (issue #430).
pub fn run_command(
    rt: &Runtime,
    cmd: ResolvedCommand<'_>,
) -> anyhow::Result<(runner::CommandResult, Option<String>)> {
    let ResolvedCommand {
        filter_cfg,
        words_consumed,
        matched_command,
        command_args,
        remaining_args,
        verbose,
    } = cmd;
    let env_overrides = build_inject_env(rt, filter_cfg);
    let env_refs: Vec<(&str, &str)> = env_overrides
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    if let Some(cfg) = filter_cfg
        && let Some(run_cmd) = &cfg.run
    {
        let run_cmd = matched_command.map_or_else(
            || String::clone(run_cmd),
            |matched_command| {
                run_command_with_consumed_prefix(
                    run_cmd,
                    matched_command,
                    command_args,
                    words_consumed,
                )
            },
        );
        let executed = runner::expand_run_command(&run_cmd, remaining_args);
        if verbose {
            eprintln!(
                "[tokf] executing: {executed}\n[tokf]   (substituted by `run` for: {})",
                command_args.join(" ")
            );
        }
        let result = runner::execute_shell_with_env(&run_cmd, remaining_args, &env_refs)?;
        Ok((result, Some(executed)))
    } else {
        // Pass argv straight through. This used to join the matched prefix with
        // spaces and let the runner split it again, which tore apart any element
        // containing one — `C:\Program Files\node.exe` became two arguments and
        // the program was reported as not found.
        //
        // words_consumed is how many argv elements the filter's `command`
        // pattern matched (0 when nothing matched); element 0 is the program
        // either way, so the rest of the matched prefix simply leads the args.
        let prefix_end = words_consumed.max(1);
        let mut args = command_args[1..prefix_end].to_vec();
        args.extend_from_slice(remaining_args);
        Ok((
            runner::execute_with_env(&command_args[0], &args, &env_refs)?,
            None,
        ))
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
pub fn try_auto_sync(rt: &Runtime) {
    use std::process::{Command, Stdio};
    use tokf::auth::credentials;
    use tokf::history::SyncConfig;
    use tokf::remote::machine;

    // Pass None for project dir: auto-sync runs in the hot path after every command,
    // so we only check the global config to avoid a filesystem scan for .tokf/config.toml.
    let config = SyncConfig::load(rt, None);
    if config.auto_sync_threshold == 0 {
        return;
    }

    if !config.upload_usage_stats.unwrap_or(false) {
        return; // None → never asked, Some(false) → opted out
    }

    if credentials::load(rt).is_none() {
        return;
    }
    if machine::load(rt).is_none() {
        return;
    }

    let Some(db_path) = rt.tracking_db_path() else {
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
            if rt.debug() {
                eprintln!("[tokf] auto-sync spawn failed: {e}");
            }
        }
    }
}

/// Stamp an event with the current project and insert it.
///
/// The single place that opens the tracking DB for a write, so every recording
/// path — filtered, generic, and pipeline capture — gets the same project
/// stamping and the same diagnostics when the DB cannot be opened. Callers
/// that need extra columns (capture sets `pipeline_tail`/`head_exit_code`)
/// build the event, adjust it, and hand it here rather than reimplementing
/// this tail.
pub fn persist_event(rt: &Runtime, event: &mut tracking::TrackingEvent) {
    let Some(path) = rt.tracking_db_path() else {
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
    event.project = current_project(rt);
    if let Err(e) = tracking::record_event(&conn, event) {
        eprintln!(
            "[tokf] tracking error (record) at {}: {e:#}",
            path.display()
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_run(
    rt: &Runtime,
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
    let command = command_args.join(" ");
    let mut event = tracking::build_event(
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
    persist_event(rt, &mut event);
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

    fn config_with_run() -> FilterConfig {
        toml::from_str(
            r#"
command = "git status"
run = "git status --porcelain=v1 -b -uall --find-renames"
"#,
        )
        .unwrap()
    }

    #[test]
    fn run_command_with_consumed_prefix_preserves_global_args_for_run_override() {
        let cfg = config_with_run();
        let command_args = vec![
            "git".to_string(),
            "-C".to_string(),
            "/tmp/repo with spaces".to_string(),
            "status".to_string(),
        ];

        let run_cmd = run_command_with_consumed_prefix(
            cfg.run.as_ref().unwrap(),
            "git status",
            &command_args,
            4,
        );

        assert_eq!(
            run_cmd,
            "git '-C' '/tmp/repo with spaces' 'status' --porcelain=v1 -b -uall --find-renames"
        );
    }

    #[test]
    fn run_command_with_consumed_prefix_keeps_plain_run_override_unchanged() {
        let cfg = config_with_run();
        let command_args = vec!["git".to_string(), "status".to_string()];

        let run_cmd = run_command_with_consumed_prefix(
            cfg.run.as_ref().unwrap(),
            "git status",
            &command_args,
            2,
        );

        assert_eq!(run_cmd, cfg.run.as_ref().unwrap().as_str());
    }

    #[test]
    fn run_command_with_consumed_prefix_ignores_nonmatching_run_override() {
        let command_args = vec![
            "git".to_string(),
            "-C".to_string(),
            "/tmp/repo".to_string(),
            "status".to_string(),
        ];

        let run_cmd =
            run_command_with_consumed_prefix("echo git status", "git status", &command_args, 4);

        assert_eq!(run_cmd, "echo git status");
    }

    #[test]
    fn run_command_with_consumed_prefix_uses_matched_array_pattern() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = ["npm test", "pnpm test"]
run = "pnpm test --reporter=dot {args}"
"#,
        )
        .unwrap();
        let command_args = vec![
            "pnpm".to_string(),
            "--dir".to_string(),
            "webapp".to_string(),
            "test".to_string(),
        ];

        let run_cmd = run_command_with_consumed_prefix(
            cfg.run.as_ref().unwrap(),
            "pnpm test",
            &command_args,
            4,
        );

        assert_eq!(
            run_cmd,
            "pnpm '--dir' 'webapp' 'test' --reporter=dot {args}"
        );
    }

    #[test]
    fn build_inject_env_empty_when_no_config() {
        let rt = Runtime::isolated();
        assert!(build_inject_env(&rt, None).is_empty());
    }

    #[test]
    fn build_inject_env_empty_when_disabled() {
        let rt = Runtime::isolated();
        let cfg = config_with_inject(false);
        assert!(build_inject_env(&rt, Some(&cfg)).is_empty());
    }

    #[test]
    fn build_inject_env_empty_when_shims_dir_missing() {
        let rt = Runtime::builder()
            .home("/nonexistent/path/tokf_test")
            .build();
        let cfg = config_with_inject(true);
        // shims_dir exists in theory but the directory doesn't exist on disk
        assert!(build_inject_env(&rt, Some(&cfg)).is_empty());
    }

    #[test]
    fn build_inject_env_returns_three_vars_when_enabled() {
        let rt = Runtime::isolated();
        let shims = rt.shims_dir().unwrap();
        std::fs::create_dir_all(&shims).unwrap();

        let cfg = config_with_inject(true);
        let env = build_inject_env(&rt, Some(&cfg));

        assert_eq!(env.len(), 3);
        assert_eq!(env[0].0, "PATH");
        assert!(env[0].1.starts_with(&shims.to_string_lossy().to_string()));
        assert_eq!(env[1].0, "TOKF_ORIGINAL_PATH");
        assert_eq!(env[2].0, "SHELL");
    }

    #[test]
    fn build_inject_env_uses_original_path_when_nested() {
        // Simulate a nested invocation: TOKF_ORIGINAL_PATH is already set.
        // Build the original value with the platform separator — hard-coding
        // `:` is the #451 bug itself, and a test that assumes it cannot observe
        // the fix.
        let original = std::env::join_paths(["/usr/bin", "/bin"].iter())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let rt = Runtime::builder().original_path(&original).build();
        let shims = rt.shims_dir().unwrap();
        std::fs::create_dir_all(&shims).unwrap();

        let cfg = config_with_inject(true);
        let env = build_inject_env(&rt, Some(&cfg));

        assert_eq!(env[1].0, "TOKF_ORIGINAL_PATH");
        assert_eq!(env[1].1, original);

        // PATH must be shims + the original entries, and must not stack the
        // shims dir twice on a nested invocation. Compare parsed entries rather
        // than a formatted string so the assertion holds on either separator.
        let entries: Vec<std::path::PathBuf> = std::env::split_paths(&env[0].1).collect();
        assert_eq!(
            entries,
            vec![
                shims,
                std::path::PathBuf::from("/usr/bin"),
                std::path::PathBuf::from("/bin"),
            ],
            "expected the shims dir once, then the original entries"
        );
    }

    // --- run_command reports what it actually executed (issue #430) ---

    #[test]
    fn run_command_reports_the_substituted_command() {
        let rt = Runtime::builder().build();
        let cfg: FilterConfig =
            toml::from_str("command = \"fake-cmd\"\nrun = \"echo substituted {args}\"").unwrap();
        let command_args = vec!["fake-cmd".to_string()];
        let remaining = vec!["extra".to_string()];

        let (result, executed) = run_command(
            &rt,
            ResolvedCommand {
                filter_cfg: Some(&cfg),
                words_consumed: 1,
                matched_command: Some("fake-cmd"),
                command_args: &command_args,
                remaining_args: &remaining,
                verbose: false,
            },
        )
        .unwrap();

        // PowerShell's `echo` is Write-Output, one line per argument; `sh`
        // emits a single space-separated line. Both substituted both words.
        assert_eq!(result.stdout.trim().replace('\n', " "), "substituted extra");
        assert_eq!(
            executed.as_deref(),
            // Args are shell-quoted: this is the literal line handed to `sh`.
            Some("echo substituted 'extra'"),
            "the fully expanded substituted command must be reported to the caller"
        );
    }

    #[test]
    fn run_command_reports_no_substitution_without_run_override() {
        let rt = Runtime::builder().build();
        let command_args = vec!["echo".to_string(), "plain".to_string()];
        let remaining = vec!["plain".to_string()];

        let (result, executed) = run_command(
            &rt,
            ResolvedCommand {
                filter_cfg: None,
                words_consumed: 0,
                matched_command: None,
                command_args: &command_args,
                remaining_args: &remaining,
                verbose: false,
            },
        )
        .unwrap();

        assert_eq!(result.stdout.trim(), "plain");
        assert_eq!(
            executed, None,
            "a verbatim run must not claim a substitution occurred"
        );
    }
}
