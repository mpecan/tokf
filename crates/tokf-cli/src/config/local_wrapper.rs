//! Detection of "local environment wrapper" commands — commands like
//! `nix develop -c <cmd>` that run an inner command in a *local* environment.
//!
//! Unlike transparent-arg commands (`ssh`/`mosh`/`slogin`, see
//! [`super::super::rewrite::transparent`]), whose trailing argv runs on a
//! remote/opaque shell and must never be inspected, a local wrapper's inner
//! command runs on the same machine. tokf strips the wrapper prefix to discover
//! which filter applies, then wraps and executes the *whole* command and
//! filters its output. See issue #403.

use tokf_hook_types::{LocalWrapperConfig, LocalWrapperRule};

use super::{ResolvedFilter, extract_basename};

/// A built-in local wrapper specification.
struct BuiltinLocalWrapper {
    /// Wrapper command basename (e.g. `nix`).
    command: &'static str,
    /// Subcommands that must immediately follow `command` (e.g. `develop`).
    /// Empty means no subcommand is required.
    subcommands: &'static [&'static str],
    /// Marker tokens whose *next* word begins the inner command.
    markers: &'static [&'static str],
}

/// Built-in local wrappers, always active unless disabled via config.
const BUILTIN_LOCAL_WRAPPERS: &[BuiltinLocalWrapper] = &[BuiltinLocalWrapper {
    command: "nix",
    subcommands: &["develop"],
    markers: &["-c", "--command"],
}];

/// Words consumed by a leading local-wrapper prefix, or `None` if `words` does
/// not start with a known local wrapper.
///
/// The consumed count spans the command, any required subcommand, and every
/// token up to and including the marker. At least one word must follow the
/// marker (a bare trailing `-c` is not a match). Built-in wrappers are honoured
/// unless `config.builtins` is `false` or their `command` appears in
/// `config.disabled`; user `config.rules` are always tried, after the built-ins.
pub fn strip_local_wrapper(words: &[&str], config: &LocalWrapperConfig) -> Option<usize> {
    if config.builtins {
        for b in BUILTIN_LOCAL_WRAPPERS {
            if config.disabled.iter().any(|d| d == b.command) {
                continue;
            }
            if let Some(n) = match_spec(words, b.command, b.subcommands, b.markers) {
                return Some(n);
            }
        }
    }
    for r in &config.rules {
        if let Some(n) = match_rule(words, r) {
            return Some(n);
        }
    }
    None
}

/// Match a user-defined rule (owned `String` fields) against `words`.
fn match_rule(words: &[&str], rule: &LocalWrapperRule) -> Option<usize> {
    let subs: Vec<&str> = rule.subcommands.iter().map(String::as_str).collect();
    let marks: Vec<&str> = rule.markers.iter().map(String::as_str).collect();
    match_spec(words, &rule.command, &subs, &marks)
}

/// Core matcher shared by built-in and user specs.
///
/// 1. `words[0]`'s basename must equal `command`.
/// 2. If `subcommands` is non-empty, `words[1]` must be one of them.
/// 3. Scan forward — tolerating any intervening tokens (flags, `.#attr`,
///    `--impure`, …) — for the first `markers` token.
/// 4. A match requires at least one word after the marker; returns
///    `marker_idx + 1` (the number of words the wrapper prefix consumes).
fn match_spec(
    words: &[&str],
    command: &str,
    subcommands: &[&str],
    markers: &[&str],
) -> Option<usize> {
    let first = words.first()?;
    if extract_basename(first) != command {
        return None;
    }

    // Position of the first word that could be a marker: after the command and
    // (if required) the subcommand.
    let scan_start = if subcommands.is_empty() {
        1
    } else {
        let sub = words.get(1)?;
        if !subcommands.contains(sub) {
            return None;
        }
        2
    };

    for (offset, word) in words[scan_start..].iter().enumerate() {
        if markers.contains(word) {
            let marker_idx = scan_start + offset;
            // Require at least one word after the marker (the inner command).
            if marker_idx + 1 < words.len() {
                return Some(marker_idx + 1);
            }
            return None;
        }
    }
    None
}

/// Try to match `words` against `filters` directly; on failure, strip one
/// local-wrapper layer and retry the inner command.
///
/// Returns `(filter, matched inner pattern, total words consumed)` where the
/// consumed count spans the wrapper prefix **and** the matched inner pattern,
/// so `command_args[..consumed]` still forms the full command prefix.
///
/// Recursion terminates: [`strip_local_wrapper`] always consumes at least two
/// words (command + marker), so `words.len()` strictly decreases each call.
pub fn match_filters_with_wrapper<'a>(
    filters: &'a [ResolvedFilter],
    words: &[&str],
    config: &LocalWrapperConfig,
) -> Option<(&'a ResolvedFilter, &'a str, usize)> {
    for filter in filters {
        if let Some((pattern, consumed)) = filter.matching_pattern(words) {
            return Some((filter, pattern, consumed));
        }
    }
    let wrapper_len = strip_local_wrapper(words, config)?;
    let (filter, pattern, inner_consumed) =
        match_filters_with_wrapper(filters, &words[wrapper_len..], config)?;
    Some((filter, pattern, wrapper_len + inner_consumed))
}

/// Returns `true` if `words` matches any of `patterns` directly, or after
/// stripping one or more local-wrapper layers.
///
/// Used by the rewrite path, which works with raw pattern strings rather than
/// [`ResolvedFilter`]s. Termination is guaranteed for the same reason as
/// [`match_filters_with_wrapper`].
pub fn patterns_match_with_wrapper(
    patterns: &[String],
    words: &[&str],
    config: &LocalWrapperConfig,
) -> bool {
    if patterns
        .iter()
        .any(|p| super::pattern_matches_prefix(p, words).is_some())
    {
        return true;
    }
    let Some(wrapper_len) = strip_local_wrapper(words, config) else {
        return false;
    };
    patterns_match_with_wrapper(patterns, &words[wrapper_len..], config)
}
