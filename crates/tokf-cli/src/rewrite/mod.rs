pub mod types;

pub(crate) mod compound;
pub(crate) mod rules;
pub(crate) mod user_config;

use std::path::PathBuf;

use crate::config;
use compound::{StrippedPipe, has_bare_pipe, split_compound, strip_env_prefix, strip_simple_pipe};
use rules::{apply_rules, should_skip};
use types::{RewriteConfig, RewriteOptions, RewriteRule};

pub use user_config::load_user_config;

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

/// Build rewrite rules by discovering installed filters (recursive walk).
///
/// For each filter pattern, generates a rewrite rule via
/// [`config::command_pattern_to_regex`].  The resulting regexes honour both
/// runtime matching behaviours:
///
/// - **Basename matching** — the first word allows an optional leading path
///   prefix, so `/usr/bin/git push` rewrites to `tokf run /usr/bin/git push`.
/// - **Transparent global flags** — flag-like tokens between pattern words are
///   tolerated, so `git -C /repo log` rewrites to `tokf run git -C /repo log`.
///
/// Handles `CommandPattern::Multiple` (one rule per pattern string) and
/// wildcards (`*` → `\S+` in the regex).
#[cfg(test)]
pub(crate) fn build_rules_from_filters(search_dirs: &[PathBuf]) -> Vec<RewriteRule> {
    build_rules_from_filters_with_options(search_dirs, &RewriteOptions::default())
}

fn build_rules_from_filters_with_options(
    search_dirs: &[PathBuf],
    options: &RewriteOptions,
) -> Vec<RewriteRule> {
    let mut rules = Vec::new();
    let mut seen_patterns: std::collections::HashSet<String> = std::collections::HashSet::new();

    let Ok(filters) = config::cache::discover_with_cache(search_dirs) else {
        return rules;
    };

    let run_prefix = if options.no_mask_exit_code {
        "tokf run --no-mask-exit-code"
    } else {
        "tokf run"
    };

    for filter in filters {
        for pattern in filter.config.command.patterns() {
            if !seen_patterns.insert(pattern.clone()) {
                continue;
            }

            let regex_str = config::command_pattern_to_regex(pattern);
            rules.push(RewriteRule {
                match_pattern: regex_str,
                replace: format!("{run_prefix} {{0}}"),
            });
        }
    }

    rules
}

/// Top-level rewrite function. Orchestrates skip check, user rules, and filter rules.
pub fn rewrite(command: &str, verbose: bool) -> String {
    rewrite_with_options(command, verbose, &RewriteOptions::default())
}

/// Top-level rewrite with explicit options (e.g. `no_mask_exit_code` for shim mode).
pub fn rewrite_with_options(command: &str, verbose: bool, options: &RewriteOptions) -> String {
    let user_config = load_user_config().unwrap_or_default();
    rewrite_with_config_and_options(
        command,
        &user_config,
        &config::default_search_dirs(),
        verbose,
        options,
    )
}

/// Collected rewrite rules passed to [`rewrite_segment`].
struct SegmentRules<'a> {
    /// Wrapper rules for task runners (tried first, before pipe handling).
    wrapper: &'a [RewriteRule],
    /// Filter-derived rules (tried after pipe handling).
    filter: &'a [RewriteRule],
    /// Options controlling `tokf run` generation (e.g. `--no-mask-exit-code`).
    options: &'a RewriteOptions,
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
fn rewrite_segment(
    segment: &str,
    rules: &SegmentRules<'_>,
    strip_pipes: bool,
    prefer_less: bool,
    verbose: bool,
) -> String {
    let (env_prefix, cmd_owned) =
        strip_env_prefix(segment).unwrap_or_else(|| (String::new(), segment.to_string()));
    let cmd = cmd_owned.as_str();

    // Wrapper rules are tried first — they inject SHELL=tokf rather than
    // wrapping with `tokf run`, so pipe stripping does not apply to them.
    let wrapper_result = apply_rules(rules.wrapper, cmd);
    if wrapper_result != cmd {
        if verbose {
            eprintln!("[tokf] wrapper rewrite: task runner shell override");
        }
        return format!("{env_prefix}{wrapper_result}");
    }

    if has_bare_pipe(cmd) {
        if strip_pipes && let Some(StrippedPipe { base, suffix }) = strip_simple_pipe(cmd) {
            let rewritten = apply_rules(rules.filter, &base);
            if rewritten != base {
                if verbose {
                    eprintln!("[tokf] stripped pipe — tokf filter provides structured output");
                }
                let injected =
                    inject_pipe_flags_with_options(&rewritten, &suffix, prefer_less, rules.options);
                return format!("{env_prefix}{injected}");
            }
        }
        if verbose {
            eprintln!("[tokf] skipping rewrite: command contains a pipe");
        }
        return segment.to_string();
    }

    let result = apply_rules(rules.filter, cmd);
    if result == cmd {
        segment.to_string()
    } else {
        format!("{env_prefix}{result}")
    }
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

fn inject_pipe_flags_with_options(
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
            let escaped = suffix.replace('\'', "'\\''");
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

/// Testable version with explicit config and search dirs (default options).
pub(crate) fn rewrite_with_config(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
) -> String {
    rewrite_with_config_and_options(
        command,
        user_config,
        search_dirs,
        verbose,
        &RewriteOptions::default(),
    )
}

/// Testable version with explicit config, search dirs, and rewrite options.
pub(crate) fn rewrite_with_config_and_options(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    let user_skip_patterns = user_config
        .skip
        .as_ref()
        .map_or(&[] as &[String], |s| &s.patterns);

    let strip_pipes = user_config.pipe.as_ref().is_none_or(|p| p.strip);
    let prefer_less = user_config.pipe.as_ref().is_some_and(|p| p.prefer_less);

    if should_skip_effective(command, user_skip_patterns) {
        return command.to_string();
    }

    // User rules run before everything — they can override built-in wrappers.
    let user_result = apply_rules(&user_config.rewrite, command);
    if user_result != command {
        return user_result;
    }

    let wrapper_rules = build_wrapper_rules();
    let filter_rules = build_rules_from_filters_with_options(search_dirs, options);
    let rules = SegmentRules {
        wrapper: &wrapper_rules,
        filter: &filter_rules,
        options,
    };
    let segments = split_compound(command);

    if segments.len() == 1 {
        return rewrite_segment(command, &rules, strip_pipes, prefer_less, verbose);
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
            let r = rewrite_segment(trimmed, &rules, strip_pipes, prefer_less, verbose);
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
mod compound_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_compound;
#[cfg(test)]
mod tests_env;
#[cfg(test)]
mod tests_pipe;
