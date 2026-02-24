pub mod types;

pub(crate) mod compound;
pub(crate) mod rules;
pub(crate) mod user_config;

use std::path::PathBuf;

use crate::config;
use compound::{StrippedPipe, has_bare_pipe, split_compound, strip_env_prefix, strip_simple_pipe};
use rules::{apply_rules, should_skip};
use types::{RewriteConfig, RewriteRule};

pub use user_config::load_user_config;

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
pub(crate) fn build_rules_from_filters(search_dirs: &[PathBuf]) -> Vec<RewriteRule> {
    let mut rules = Vec::new();
    let mut seen_patterns: std::collections::HashSet<String> = std::collections::HashSet::new();

    let Ok(filters) = config::cache::discover_with_cache(search_dirs) else {
        return rules;
    };

    for filter in filters {
        for pattern in filter.config.command.patterns() {
            if !seen_patterns.insert(pattern.clone()) {
                continue;
            }

            let regex_str = config::command_pattern_to_regex(pattern);
            rules.push(RewriteRule {
                match_pattern: regex_str,
                replace: "tokf run {0}".to_string(),
            });
        }
    }

    rules
}

/// Top-level rewrite function. Orchestrates skip check, user rules, and filter rules.
pub fn rewrite(command: &str, verbose: bool) -> String {
    let user_config = load_user_config().unwrap_or_default();
    rewrite_with_config(
        command,
        &user_config,
        &config::default_search_dirs(),
        verbose,
    )
}

/// Rewrite a single command segment, handling pipe stripping and env var
/// prefixes when appropriate.
///
/// Leading `KEY=VALUE` assignments are stripped before matching so that
/// `FOO=bar git status` rewrites to `FOO=bar tokf run git status` rather than
/// passing through unchanged. The env prefix is preserved in the output and
/// applied to the command that actually runs.
///
/// If the (env-stripped) segment has a bare pipe to a simple target (tail,
/// head, grep) and the base command matches a tokf filter, the pipe is also
/// stripped and `--baseline-pipe` is injected — unless `strip_pipes` is false.
/// When `prefer_less` is true, `--prefer-less` is also injected so that at
/// runtime the smaller of filtered vs piped output is used.
fn rewrite_segment(
    segment: &str,
    filter_rules: &[RewriteRule],
    strip_pipes: bool,
    prefer_less: bool,
    verbose: bool,
) -> String {
    let (env_prefix, cmd_owned) =
        strip_env_prefix(segment).unwrap_or_else(|| (String::new(), segment.to_string()));
    let cmd = cmd_owned.as_str();

    if has_bare_pipe(cmd) {
        if strip_pipes && let Some(StrippedPipe { base, suffix }) = strip_simple_pipe(cmd) {
            let rewritten = apply_rules(filter_rules, &base);
            if rewritten != base {
                if verbose {
                    eprintln!("[tokf] stripped pipe — tokf filter provides structured output");
                }
                let injected = inject_pipe_flags(&rewritten, &suffix, prefer_less);
                return format!("{env_prefix}{injected}");
            }
        }
        if verbose {
            eprintln!("[tokf] skipping rewrite: command contains a pipe");
        }
        return segment.to_string();
    }

    let result = apply_rules(filter_rules, cmd);
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
fn inject_pipe_flags(rewritten: &str, suffix: &str, prefer_less: bool) -> String {
    rewritten.strip_prefix("tokf run ").map_or_else(
        || rewritten.to_string(),
        |rest| {
            let escaped = suffix.replace('\'', "'\\''");
            let prefer_flag = if prefer_less { " --prefer-less" } else { "" };
            format!("tokf run --baseline-pipe '{escaped}'{prefer_flag} {rest}")
        },
    )
}

/// Check if a command should be skipped, considering both the raw form and the
/// env-prefix-stripped form.
///
/// User-defined skip patterns operate on the full segment (env prefix included),
/// giving users explicit control over what they skip. The built-in patterns
/// (`^tokf `, `<<`) are also checked on the env-stripped command so that
/// `DEBUG=1 tokf run git status` is correctly identified as already-rewritten
/// and not double-wrapped.
fn should_skip_effective(command: &str, user_patterns: &[String]) -> bool {
    if should_skip(command, user_patterns) {
        return true;
    }
    // Only built-in patterns (no user patterns) are checked on the stripped form.
    strip_env_prefix(command).is_some_and(|(_, cmd)| should_skip(&cmd, &[]))
}

/// Testable version with explicit config and search dirs.
pub(crate) fn rewrite_with_config(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
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

    // User rules run before the pipe guard so they can explicitly wrap piped commands.
    let user_result = apply_rules(&user_config.rewrite, command);
    if user_result != command {
        return user_result;
    }

    let filter_rules = build_rules_from_filters(search_dirs);
    let segments = split_compound(command);

    if segments.len() == 1 {
        return rewrite_segment(command, &filter_rules, strip_pipes, prefer_less, verbose);
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
            let r = rewrite_segment(trimmed, &filter_rules, strip_pipes, prefer_less, verbose);
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
