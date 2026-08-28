pub mod types;

pub(crate) mod bash_ast;
pub mod capture;
pub(crate) mod rules;
pub(crate) mod transparent;
pub(crate) mod user_config;

use std::path::PathBuf;

use crate::config;
use bash_ast::{split_compound, strip_env_prefix};
use rules::{apply_rules, should_skip};
use types::{PipeConfig, RewriteConfig, RewriteOptions, RewriteRule};

pub use user_config::{load_local_wrapper_config, load_user_config};

use crate::runtime::Runtime;

/// Built-in wrapper rules for task runners that support shell overrides.
///
/// These rewrite the command to inject tokf as the task runner's shell, so each
/// recipe line is individually matched and filtered.  The outer command runs
/// directly (not via `tokf run`) — its exit code flows through unmodified.
///
/// Note: the replacement strings use the bare command name (`make`, `just`)
/// rather than preserving the original path prefix.  `/usr/bin/make check`
/// rewrites to `make SHELL=tokf check`.  This is intentional — the user's
/// `$PATH` resolves the command, and injecting `SHELL=tokf` into a full-path
/// invocation would look unusual.
///
/// Users can override these via `[[rewrite]]` entries in `rewrites.toml`.
const BUILTIN_WRAPPERS: &[(&str, &str)] = &[
    // make: override $(SHELL) so recipe lines run as `tokf -c 'line'`
    (r"^(?:[^\s]*/)?make(\s.*)?$", "make SHELL=tokf{1}"),
    // just: use --shell flag to route recipe lines through `tokf -cu 'line'`
    (
        r"^(?:[^\s]*/)?just(\s.*)?$",
        "just --shell tokf --shell-arg -cu{1}",
    ),
];

/// Build `RewriteRule` entries from the built-in wrapper table.
fn build_wrapper_rules() -> Vec<RewriteRule> {
    BUILTIN_WRAPPERS
        .iter()
        .map(|(pattern, replace)| RewriteRule {
            match_pattern: (*pattern).to_string(),
            replace: (*replace).to_string(),
        })
        .collect()
}

/// Collect raw filter pattern strings from all discovered filters.
///
/// These patterns are matched using [`config::pattern_matches_prefix`] — the
/// same authoritative matching logic used by `tokf run` and `tokf which` — so
/// that `tokf -c` (shell mode) and `tokf rewrite` produce identical results.
fn collect_filter_patterns(rt: &Runtime, search_dirs: &[PathBuf], no_cache: bool) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let discovered = if no_cache {
        config::discover_all_filters(search_dirs)
    } else {
        config::cache::discover_with_cache(rt, search_dirs)
    };
    let Ok(filters) = discovered else {
        return patterns;
    };
    for filter in filters {
        for pattern in filter.config.command.patterns() {
            let owned = pattern.clone();
            if seen.insert(owned.clone()) {
                patterns.push(owned);
            }
        }
    }
    patterns
}

/// Try to match a command against filter patterns using the authoritative
/// [`config::pattern_matches_prefix`] logic.  Returns a `tokf run` invocation
/// if matched.
///
/// A command wrapped in a local environment wrapper (e.g.
/// `nix develop -c cargo test`) matches when its inner command does. The wrap
/// is applied to the **whole** command (`tokf run nix develop -c cargo test`),
/// not the inner part — tokf is the parent process and filters the wrapper's
/// combined output, so nothing needs `tokf` on `PATH` inside the wrapped
/// environment. See issue #403.
fn try_filter_match(
    cmd: &str,
    patterns: &[String],
    options: &RewriteOptions,
    local_wrapper: &types::LocalWrapperConfig,
) -> Option<String> {
    let words: Vec<&str> = cmd.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    if !config::local_wrapper::patterns_match_with_wrapper(patterns, &words, local_wrapper) {
        return None;
    }
    let prefix = if options.no_mask_exit_code {
        "tokf run --no-mask-exit-code"
    } else {
        "tokf run"
    };
    Some(format!("{prefix} {cmd}"))
}

/// Match a segment against filter patterns using the fields collected in
/// [`SegmentRules`]. Thin wrapper over [`try_filter_match`].
pub(super) fn segment_filter_match(cmd: &str, rules: &SegmentRules<'_>) -> Option<String> {
    try_filter_match(
        cmd,
        rules.filter_patterns,
        rules.options,
        rules.local_wrapper,
    )
}

/// Top-level rewrite function. Orchestrates skip check, user rules, and filter rules.
pub fn rewrite(rt: &Runtime, command: &str, verbose: bool) -> String {
    rewrite_with_options(rt, command, verbose, &RewriteOptions::default())
}

/// Top-level rewrite with explicit options (e.g. `no_mask_exit_code` for shim mode).
pub fn rewrite_with_options(
    rt: &Runtime,
    command: &str,
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    let user_config = load_user_config(rt).unwrap_or_default();
    rewrite_with_config_and_options(
        RewriteCtx {
            rt,
            user_config: &user_config,
            search_dirs: &config::default_search_dirs(rt),
            // `--no-cache` is not threaded through the `tokf rewrite` subcommand
            // or the `tokf -c` shell path yet (see #431 follow-up); those callers
            // always use the cache. The hook path sets this explicitly.
            no_cache: false,
        },
        command,
        verbose,
        options,
    )
}

/// Collected rewrite rules passed to [`rewrite_segment`].
pub(super) struct SegmentRules<'a> {
    /// Wrapper rules for task runners (tried first, before pipe handling).
    pub(super) wrapper: &'a [RewriteRule],
    /// Raw filter pattern strings matched via `pattern_matches_prefix`.
    pub(super) filter_patterns: &'a [String],
    /// Local environment wrappers (e.g. `nix develop -c`) to unwrap when
    /// matching filter patterns.
    pub(super) local_wrapper: &'a types::LocalWrapperConfig,
    /// Options controlling `tokf run` generation (e.g. `--no-mask-exit-code`).
    pub(super) options: &'a RewriteOptions,
    /// User `[pipe]` settings: pipe stripping, prefer-less, and capture.
    pub(super) pipe: &'a PipeConfig,
    /// True when the *whole* command already handles pipeline status itself
    /// (`set -o pipefail`, `${PIPESTATUS[0]}`). Capture is declined for these:
    /// the author has dealt with it, so a mismatch report would be redundant.
    /// Deliberately not a `should_skip` pattern — that would also cost
    /// `set -o pipefail; cargo test` its filter, which is unrelated.
    pub(super) pipefail_handled: bool,
    /// When true, log to stderr when the bash parser fails to parse a command.
    pub(super) log_parse_failures: bool,
}

/// Rewrite a single command segment, handling pipe stripping and env var
/// prefixes when appropriate.
///
/// Leading `KEY=VALUE` assignments are stripped before matching so that
/// `FOO=bar git status` rewrites to `FOO=bar tokf run git status` rather than
/// passing through unchanged. The env prefix is preserved in the output and
/// applied to the command that actually runs.
///
/// **Wrapper rules** (for task runners like `make` and `just`) are tried first,
/// before pipe handling.  Wrapper rewrites inject tokf as the task runner's
/// shell, and pipe stripping is not applicable to them.
///
/// If the (env-stripped) segment has a bare pipe to a simple target (tail,
/// head, grep) and the base command matches a tokf filter, the pipe is also
/// stripped and `--baseline-pipe` is injected — unless `strip_pipes` is false.
/// When `prefer_less` is true, `--prefer-less` is also injected so that at
/// runtime the smaller of filtered vs piped output is used.
fn rewrite_segment(segment: &str, sep: &str, rules: &SegmentRules<'_>, verbose: bool) -> String {
    // Parse once — reuse the AST for env_prefix, pipe detection, and stripping.
    let parsed = bash_ast::ParsedCommand::parse(segment);
    if parsed.is_none() && rules.log_parse_failures {
        eprintln!(
            "[tokf] debug: bash parser failed to parse command, falling back to string matching: {segment}"
        );
    }
    let (env_prefix, cmd_owned) = parsed
        .as_ref()
        .and_then(bash_ast::ParsedCommand::env_prefix)
        .unwrap_or_else(|| (String::new(), segment.to_string()));
    let cmd = cmd_owned.as_str();

    // Parse the env-stripped command for pipe analysis (reuse if no env prefix).
    let cmd_parsed = if env_prefix.is_empty() {
        parsed
    } else {
        let p = bash_ast::ParsedCommand::parse(cmd);
        if p.is_none() && rules.log_parse_failures {
            eprintln!("[tokf] debug: bash parser failed to parse env-stripped command: {cmd}");
        }
        p
    };

    if cmd_parsed
        .as_ref()
        .is_some_and(bash_ast::ParsedCommand::has_bare_pipe)
    {
        return capture::rewrite_piped_segment(
            capture::PipedSegment {
                segment,
                sep,
                cmd,
                env_prefix: &env_prefix,
                parsed: cmd_parsed.as_ref(),
            },
            rules,
            verbose,
        );
    }

    if let Some(wrapped) = apply_rules(rules.wrapper, cmd) {
        return wrapper_rewrite(&env_prefix, &wrapped, verbose);
    }

    segment_filter_match(cmd, rules).map_or_else(
        || segment.to_string(),
        |result| format!("{env_prefix}{result}"),
    )
}

/// True when the command already propagates pipeline status itself.
///
/// A substring scan rather than an AST predicate, which is deliberately
/// conservative: `git commit -m "fix pipefail handling"` also declines
/// capture. Erring toward leaving a command alone is the safe direction, and
/// the alternative would need `set -o` and `${PIPESTATUS[…]}` modelled per
/// segment for no gain on any realistic command.
fn handles_pipe_status(command: &str) -> bool {
    command.contains("pipefail") || command.contains("PIPESTATUS")
}

/// Emit a wrapper rewrite (`make SHELL=tokf …`), with its verbose note.
///
/// Shared because both the piped and unpiped paths end here when a task-runner
/// rule claims the segment.
pub(super) fn wrapper_rewrite(env_prefix: &str, wrapped: &str, verbose: bool) -> String {
    if verbose {
        eprintln!("[tokf] wrapper rewrite: task runner shell override");
    }
    format!("{env_prefix}{wrapped}")
}

/// Escape a fragment for embedding inside single quotes in generated shell.
///
/// Both `--baseline-pipe` and `--pipe-through` hand a shell fragment through
/// `tokf run` to `sh -c`, and both need the `'\''` idiom so a quoted pattern
/// like `grep -E 'fail|error'` survives the round trip intact.
pub(super) fn single_quote(fragment: &str) -> String {
    fragment.replace('\'', "'\\''")
}

/// Insert `--baseline-pipe '<suffix>'` (and optionally `--prefer-less`) after
/// `tokf run` in the rewritten command.
///
/// Single quotes in the suffix are escaped with the `'\''` idiom so the
/// generated shell command remains valid (e.g. `grep -E 'fail|error'`).
#[cfg(test)]
fn inject_pipe_flags(rewritten: &str, suffix: &str, prefer_less: bool) -> String {
    inject_pipe_flags_with_options(rewritten, suffix, prefer_less, &RewriteOptions::default())
}

pub(super) fn inject_pipe_flags_with_options(
    rewritten: &str,
    suffix: &str,
    prefer_less: bool,
    options: &RewriteOptions,
) -> String {
    rewritten.strip_prefix("tokf run ").map_or_else(
        || rewritten.to_string(),
        |rest| {
            // rest may start with --no-mask-exit-code from the rule template;
            // strip it so we don't duplicate the flag when options also requests it.
            let rest = rest.strip_prefix("--no-mask-exit-code ").unwrap_or(rest);
            let escaped = single_quote(suffix);
            let prefer_flag = if prefer_less { " --prefer-less" } else { "" };
            let mask_flag = if options.no_mask_exit_code {
                " --no-mask-exit-code"
            } else {
                ""
            };
            format!("tokf run{mask_flag} --baseline-pipe '{escaped}'{prefer_flag} {rest}")
        },
    )
}

/// Check if a command should be skipped, considering both the raw form and the
/// env-prefix-stripped form.
///
/// User-defined skip patterns operate on the full segment (env prefix included),
/// giving users explicit control over what they skip. The built-in patterns
/// (`^tokf `, top-level heredoc) are also checked on the env-stripped command so that
/// `DEBUG=1 tokf run git status` is correctly identified as already-rewritten
/// and not double-wrapped.
fn should_skip_effective(command: &str, user_patterns: &[String]) -> bool {
    if should_skip(command, user_patterns) {
        return true;
    }
    // Only built-in patterns (no user patterns) are checked on the stripped form.
    strip_env_prefix(command).is_some_and(|(_, cmd)| should_skip(&cmd, &[]))
}

/// Everything a rewrite needs from its surroundings, bundled so the rewrite
/// entry points stay within the argument limit.
#[derive(Clone, Copy)]
pub(crate) struct RewriteCtx<'a> {
    pub rt: &'a Runtime,
    pub user_config: &'a RewriteConfig,
    pub search_dirs: &'a [PathBuf],
    /// Bypass the on-disk filter discovery cache (honours `--no-cache`).
    /// Kept in the context — rather than as a positional `bool` argument next to
    /// `verbose` — so call sites name the flag and can't silently swap the two
    /// (the exact footgun behind #431).
    pub no_cache: bool,
}

/// Testable version with explicit config, search dirs, and rewrite options.
pub(crate) fn rewrite_with_config_and_options(
    ctx: RewriteCtx<'_>,
    command: &str,
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    let user_config = ctx.user_config;
    let user_skip_patterns = user_config
        .skip
        .as_ref()
        .map_or(&[] as &[String], |s| &s.patterns);

    if should_skip_effective(command, user_skip_patterns) {
        return command.to_string();
    }

    let transparent_extras: &[String] = user_config
        .transparent
        .as_ref()
        .map_or(&[] as &[String], |t| &t.commands);

    // User rules run before everything — they can override built-in wrappers.
    // Skipped when *any* compound segment is a transparent-arg invocation
    // (#338): regex rewrites operate on the full command string, so even an
    // ssh segment buried behind a `cd … &&` could have text spliced into its
    // opaque payload. Argv-preserving wraps below still apply per-segment.
    if !transparent::any_segment_is_transparent(command, transparent_extras)
        && let Some(user_result) = apply_rules(&user_config.rewrite, command)
    {
        return user_result;
    }

    let wrapper_rules = build_wrapper_rules();
    let filter_patterns = collect_filter_patterns(ctx.rt, ctx.search_dirs, ctx.no_cache);
    let local_wrapper = user_config.local_wrapper.clone().unwrap_or_default();
    let log_parse_failures = user_config
        .debug
        .as_ref()
        .is_some_and(|d| d.log_parse_failures);
    let mut pipe_cfg = user_config.pipe.clone().unwrap_or_default();
    // `TOKF_PIPE_CAPTURE` wins over the file in both directions: capture
    // changes how commands execute, so turning it off must not require an edit.
    if let Some(override_capture) = ctx.rt.pipe_capture() {
        pipe_cfg.capture = override_capture;
    }
    let rules = SegmentRules {
        wrapper: &wrapper_rules,
        filter_patterns: &filter_patterns,
        local_wrapper: &local_wrapper,
        options,
        pipe: &pipe_cfg,
        // Only consulted on the capture path, so it is not computed at all
        // in the default configuration.
        pipefail_handled: pipe_cfg.capture && handles_pipe_status(command),
        log_parse_failures,
    };
    let segments = split_compound(command);

    if segments.len() == 1 {
        return rewrite_segment(command, "", &rules, verbose);
    }

    // Compound command: rewrite each segment independently so every sub-command
    // that has a matching filter is wrapped, not just the first one.
    let mut changed = false;
    let mut out = String::with_capacity(command.len() + segments.len() * 9);
    for (seg, sep) in &segments {
        let trimmed = seg.trim();
        let rewritten = if trimmed.is_empty() || should_skip_effective(trimmed, user_skip_patterns)
        {
            trimmed.to_string()
        } else {
            let r = rewrite_segment(trimmed, sep, &rules, verbose);
            if r != trimmed {
                changed = true;
            }
            r
        };
        out.push_str(&rewritten);
        out.push_str(sep);
    }
    if changed { out } else { command.to_string() }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod bash_ast_multibyte_tests;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod bash_ast_tests;
/// Test helpers: run a rewrite with explicit config against a freshly isolated
/// runtime, so every test gets its own directories with no setup and no shared
/// state to collide over.
#[cfg(test)]
pub(crate) fn rewrite_isolated(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
) -> String {
    rewrite_isolated_with_options(
        command,
        user_config,
        search_dirs,
        verbose,
        &RewriteOptions::default(),
    )
}

#[cfg(test)]
pub(crate) fn rewrite_isolated_with_options(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    let rt = Runtime::isolated();
    rewrite_with_config_and_options(
        RewriteCtx {
            rt: &rt,
            user_config,
            search_dirs,
            no_cache: false,
        },
        command,
        verbose,
        options,
    )
}

#[cfg(test)]
pub(crate) fn collect_filter_patterns_isolated(search_dirs: &[PathBuf]) -> Vec<String> {
    let rt = Runtime::isolated();
    collect_filter_patterns(&rt, search_dirs, false)
}

#[cfg(test)]
mod proptest_rewrite;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_capture;
#[cfg(test)]
mod tests_compound;
#[cfg(test)]
mod tests_env;
#[cfg(test)]
mod tests_local_wrapper;
#[cfg(test)]
mod tests_pipe;
#[cfg(test)]
mod tests_transparent;
