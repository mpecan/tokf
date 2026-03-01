mod aggregate;
pub mod chunk;
mod cleanup;
mod dedup;
mod extract;
mod group;
#[cfg(feature = "lua")]
pub mod lua;
mod match_output;
mod parse;
mod replace;
pub mod section;
mod skip;
mod template;

use regex::Regex;

use tokf_common::config::types::{FilterConfig, OutputBranch};

use crate::CommandResult;

use self::section::SectionMap;

/// Compile a list of regex pattern strings, silently dropping invalid ones.
pub(crate) fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
    patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
}

/// Runtime options for the filter pipeline, passed from CLI flags.
#[derive(Debug, Clone, Default)]
pub struct FilterOptions {
    /// Preserve ANSI color codes in filtered output. When true, tokf strips
    /// ANSI internally for pattern matching (skip/keep/dedup) but restores
    /// original colored lines in the final output.
    ///
    /// **Limitations:** color passthrough only applies to the skip/keep/dedup
    /// pipeline (stages 2–2.5). The `match_output`, `parse`, and `lua_script`
    /// stages operate on clean text and are unaffected by this flag.
    pub preserve_color: bool,
}

/// The result of applying a filter to command output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterResult {
    pub output: String,
}

/// Apply a filter configuration to a command result.
///
/// Processing order:
///
/// ```text
/// 1.   match_output  — substring check, first match wins
/// 1.5. [[replace]]   — per-line regex transformations
/// 1.6. strip_ansi / trim_lines — per-line cleanup
/// 2.   skip/keep     — top-level pre-filtering
/// 2.5. dedup         — collapse duplicate lines
/// 2b.  lua_script    — escape hatch (if configured)
/// 3.   parse         — alternative structured path
/// 4.   sections      — state-machine line collection
/// 5.   select branch — exit code 0 → on_success, else on_failure
/// 6.   apply branch  — render output or fallback
/// 6.5. strip_empty_lines / collapse_empty_lines — post-process output
/// ```
/// Dual-track line storage for color passthrough mode.
///
/// When `--preserve-color` is active, `display` holds the original colored
/// lines while `clean` holds ANSI-stripped lines for pattern matching. When
/// color mode is off, only `clean` is populated (same as previous behavior).
struct RawLines {
    /// Lines for pattern matching (ANSI-stripped when color mode is active).
    clean: Vec<String>,
    /// Original display lines with ANSI codes preserved. `None` when color
    /// passthrough is off.
    display: Option<Vec<String>>,
}

/// Apply stage 1.5 + 1.6 pre-filter transforms (`replace`, `strip_ansi`, `trim_lines`).
///
/// When `preserve_color` is true, always strips ANSI for clean lines and keeps
/// the original colored lines in `display` for final output restoration.
fn build_raw_lines(combined: &str, config: &FilterConfig, opts: &FilterOptions) -> RawLines {
    let initial: Vec<&str> = combined.lines().collect();
    let after_replace = if config.replace.is_empty() {
        initial.iter().map(ToString::to_string).collect()
    } else {
        replace::apply_replace(&config.replace, &initial)
    };

    if opts.preserve_color {
        let display = after_replace.clone();
        let clean: Vec<String> = after_replace
            .into_iter()
            .map(|line| {
                let stripped = cleanup::strip_ansi_from(&line);
                if config.trim_lines {
                    stripped.trim().to_string()
                } else {
                    stripped
                }
            })
            .collect();
        RawLines {
            clean,
            display: Some(display),
        }
    } else if config.strip_ansi || config.trim_lines {
        let refs: Vec<&str> = after_replace.iter().map(String::as_str).collect();
        RawLines {
            clean: cleanup::apply_line_cleanup(config, &refs),
            display: None,
        }
    } else {
        RawLines {
            clean: after_replace,
            display: None,
        }
    }
}

/// Map surviving clean-line references back to their display counterparts.
///
/// `survivors` are `&str` references into `clean` (via `as_str()`), preserved
/// through skip/keep/dedup which only filter without reordering. We scan
/// `clean` in order, matching by pointer identity, and collect the
/// corresponding `display` line for each match.
fn restore_display_lines(clean: &[String], display: &[String], survivors: &[&str]) -> String {
    let mut result = Vec::with_capacity(survivors.len());
    let mut si = 0;
    for (i, c) in clean.iter().enumerate() {
        if si >= survivors.len() {
            break;
        }
        if std::ptr::eq(
            std::ptr::from_ref::<str>(c.as_str()),
            std::ptr::from_ref::<str>(survivors[si]),
        ) {
            result.push(display[i].as_str());
            si += 1;
        }
    }
    result.join("\n")
}

/// Load and run a Lua script with the given sandbox limits.
///
/// Returns `Some(output)` when the script replaces output, `None` for
/// passthrough or on error (errors are printed to stderr).
#[cfg(feature = "lua")]
fn run_lua(
    script_cfg: &tokf_common::config::types::ScriptConfig,
    text: &str,
    exit_code: i32,
    args: &[String],
    limits: &lua::SandboxLimits,
) -> Option<String> {
    match lua::load_source(script_cfg) {
        Ok(source) => match lua::run_lua_script_sandboxed(&source, text, exit_code, args, limits) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("[tokf] lua script error: {e:#}");
                None
            }
        },
        Err(e) => {
            eprintln!("[tokf] lua script error: {e:#}");
            None
        }
    }
}

pub fn apply(
    config: &FilterConfig,
    result: &CommandResult,
    args: &[String],
    opts: &FilterOptions,
) -> FilterResult {
    #[cfg(feature = "lua")]
    let lua_limits = lua::SandboxLimits::default();
    #[cfg(feature = "lua")]
    return apply_internal(config, result, args, opts, &lua_limits);
    #[cfg(not(feature = "lua"))]
    apply_internal(config, result, args, opts)
}

/// Apply a filter with explicit Lua sandbox limits.
///
/// Identical to [`apply`] except the caller provides `SandboxLimits` instead
/// of using the defaults. Use this for server-side validation or when
/// processing untrusted filter configs.
#[cfg(feature = "lua")]
pub fn apply_sandboxed(
    config: &FilterConfig,
    result: &CommandResult,
    args: &[String],
    opts: &FilterOptions,
    lua_limits: &lua::SandboxLimits,
) -> FilterResult {
    apply_internal(config, result, args, opts, lua_limits)
}

/// Shared filter pipeline implementation.
///
/// All filter stages run through this single function. The optional
/// `lua_limits` parameter controls Lua sandbox constraints; `apply()`
/// passes defaults while `apply_sandboxed()` passes caller-provided limits.
#[allow(clippy::too_many_lines)]
fn apply_internal(
    config: &FilterConfig,
    result: &CommandResult,
    args: &[String],
    opts: &FilterOptions,
    #[cfg(feature = "lua")] lua_limits: &lua::SandboxLimits,
) -> FilterResult {
    // 1. match_output short-circuit
    if let Some(rule) = match_output::find_matching_rule(&config.match_output, &result.combined) {
        let output = match_output::render_output(&rule.output, &rule.contains, &result.combined);
        return FilterResult {
            output: cleanup::post_process_output(config, output),
        };
    }

    // 1.5 + 1.6. Replace + per-line cleanup (strip_ansi, trim_lines)
    let raw = build_raw_lines(&result.combined, config, opts);
    let raw_lines: Vec<&str> = raw.clean.iter().map(String::as_str).collect();

    // 2. Top-level skip/keep pre-filtering
    let lines = skip::apply_skip(&config.skip, &raw_lines);
    let lines = skip::apply_keep(&config.keep, &lines);

    // 2.5. Dedup
    let lines = if config.dedup {
        dedup::apply_dedup(&lines, config.dedup_window)
    } else {
        lines
    };

    // 2b. Lua script escape hatch (sandboxed)
    #[cfg(feature = "lua")]
    if let Some(ref script_cfg) = config.lua_script {
        let clean_text = lines.join("\n");
        if let Some(output) = run_lua(script_cfg, &clean_text, result.exit_code, args, lua_limits) {
            return FilterResult {
                output: cleanup::post_process_output(config, output),
            };
        }
    }

    // 3. If parse exists → parse+output pipeline
    if let Some(ref parse_config) = config.parse {
        let parse_result = parse::run_parse(parse_config, &lines);
        let output_config = config.output.clone().unwrap_or_default();
        let output = parse::render_output(&output_config, &parse_result);
        return FilterResult {
            output: cleanup::post_process_output(config, output),
        };
    }

    // 4. Collect sections and chunks (both run on raw output — they need
    //    structural markers like blank lines that skip patterns remove).
    //    DESIGN NOTE: section enter/exit regexes match against the original,
    //    unmodified lines. If the command emits ANSI codes in marker lines,
    //    set `strip_ansi = true` AND write patterns that match the raw text,
    //    or configure the command to disable color (e.g. `--no-color`).
    let has_sections = !config.section.is_empty();
    let needs_raw_lines = has_sections || !config.chunk.is_empty();
    let raw_lines: Vec<&str> = if needs_raw_lines {
        result.combined.lines().collect()
    } else {
        Vec::new()
    };

    let sections = if has_sections {
        section::collect_sections(&config.section, &raw_lines)
    } else {
        SectionMap::new()
    };

    // Restore display lines for color mode, or join clean lines.
    let pre_filtered = if let Some(ref display) = raw.display {
        restore_display_lines(&raw.clean, display, &lines)
    } else {
        lines.join("\n")
    };

    let chunks = if config.chunk.is_empty() {
        template::ChunkMap::new()
    } else {
        chunk::process_chunks(&config.chunk, &raw_lines)
    };

    // 5. Select branch by exit code
    let branch = select_branch(config, result.exit_code);
    let output = branch.map_or_else(
        || apply_fallback(config, &pre_filtered),
        |b| {
            apply_branch(b, &pre_filtered, &sections, &chunks, has_sections)
                .unwrap_or_else(|| apply_fallback(config, &pre_filtered))
        },
    );

    FilterResult {
        output: cleanup::post_process_output(config, output),
    }
}

/// Select the output branch based on exit code.
/// Exit code 0 → `on_success`, anything else → `on_failure`.
const fn select_branch(config: &FilterConfig, exit_code: i32) -> Option<&OutputBranch> {
    if exit_code == 0 {
        config.on_success.as_ref()
    } else {
        config.on_failure.as_ref()
    }
}

/// Apply a branch's processing rules to the combined output.
///
/// When `has_sections` is true and the branch has an output template,
/// the template is rendered with aggregation vars and section data.
/// Returns `None` when sections were expected but collected nothing
/// (signals: use fallback).
///
/// Processing order (non-section path):
/// 1. Fixed `output` string → return immediately
/// 2. `tail` / `head` truncation
/// 3. `skip` patterns
/// 4. `extract` rule
/// 5. Remaining lines joined with `\n`
fn apply_branch(
    branch: &OutputBranch,
    combined: &str,
    sections: &SectionMap,
    chunks: &template::ChunkMap,
    has_sections: bool,
) -> Option<String> {
    // 1. Aggregation — merge singular `aggregate` + plural `aggregates`
    let mut all_rules: Vec<&tokf_common::config::types::AggregateRule> =
        branch.aggregates.iter().collect();
    if let Some(ref single) = branch.aggregate {
        all_rules.push(single);
    }
    let vars = if all_rules.is_empty() {
        std::collections::HashMap::new()
    } else {
        let owned_rules: Vec<tokf_common::config::types::AggregateRule> =
            all_rules.into_iter().cloned().collect();
        aggregate::run_aggregates(&owned_rules, sections)
    };

    // 2. Output template
    if let Some(ref output_tmpl) = branch.output {
        if has_sections {
            let any_collected = sections
                .values()
                .any(|s| !s.lines.is_empty() || !s.blocks.is_empty());
            if !any_collected {
                return None; // sections expected but empty → fallback
            }
        }
        let mut vars = vars;
        vars.insert("output".to_string(), combined.to_string());
        return Some(template::render_template(
            output_tmpl,
            &vars,
            sections,
            chunks,
        ));
    }

    // Non-template path (tail/head/skip/extract)
    let mut lines: Vec<&str> = combined.lines().collect();

    if let Some(tail) = branch.tail
        && lines.len() > tail
    {
        lines = lines.split_off(lines.len() - tail);
    }
    if let Some(head) = branch.head {
        lines.truncate(head);
    }

    lines = skip::apply_skip(&branch.skip, &lines);

    if let Some(ref rule) = branch.extract {
        return Some(extract::apply_extract(rule, &lines));
    }

    Some(lines.join("\n"))
}

/// Fallback when no branch matches or sections collected nothing.
fn apply_fallback(config: &FilterConfig, combined: &str) -> String {
    if let Some(ref fb) = config.fallback
        && let Some(tail) = fb.tail
    {
        let lines: Vec<&str> = combined.lines().collect();
        if lines.len() > tail {
            return lines[lines.len() - tail..].join("\n");
        }
    }
    combined.to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests_chunk;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_color;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_pipeline;
