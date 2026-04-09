mod aggregate;
pub mod chunk;
mod cleanup;
mod dedup;
mod extract;
mod group;
pub mod json;
#[cfg(feature = "lua")]
pub mod lua;
mod match_output;
mod parse;
mod replace;
pub mod section;
mod skip;
mod template;
mod tree;

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
    /// stages operate on clean text and are unaffected by this flag. The
    /// `[tree]` transform (stage 2.6) also bypasses color restoration when
    /// active — tree-rendered lines are synthesized from path components,
    /// so per-line ANSI color spans don't survive structural rearrangement.
    pub preserve_color: bool,
}

/// The result of applying a filter to command output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterResult {
    pub output: String,
}

/// Pipeline state collected before branch rendering.
///
/// Groups the context built during `apply_internal` that `apply_branch` needs,
/// replacing what was previously 6 positional parameters.
struct BranchContext<'a> {
    sections: &'a SectionMap,
    chunks: &'a template::ChunkMap,
    has_sections: bool,
    has_json: bool,
    json_parsed: bool,
    json_vars: &'a std::collections::HashMap<String, String>,
    top_level_tail: Option<usize>,
    top_level_head: Option<usize>,
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
    // When strip_ansi is enabled, match against cleaned text for consistency.
    let match_text = if config.strip_ansi {
        cleanup::strip_ansi_from(&result.combined)
    } else {
        result.combined.clone()
    };
    if let Some((rule, needle)) =
        match_output::find_matching_rule(&config.match_output, &match_text)
    {
        let output = match_output::render_output(&rule.output, &needle, &match_text);
        return FilterResult {
            output: finalize_output(config, output),
        };
    }

    // 1.5 + 1.6. Replace + per-line cleanup (strip_ansi, trim_lines)
    let raw = build_raw_lines(&result.combined, config, opts);
    let clean_lines: Vec<&str> = raw.clean.iter().map(String::as_str).collect();

    // 2. Top-level skip/keep pre-filtering
    let lines = skip::apply_skip(&config.skip, &clean_lines);
    let lines = skip::apply_keep(&config.keep, &lines);

    // 2.5. Dedup
    let lines = if config.dedup {
        dedup::apply_dedup(&lines, config.dedup_window)
    } else {
        lines
    };

    // 2.6. Tree transform — restructures path-list output into a directory
    // tree. Returns Some(rendered) when engagement gates pass, None when
    // they don't (caller treats None as "use original lines unchanged").
    //
    // **Precedence:** when [parse] is also configured, parse wins and tree
    // is silently skipped (parse early-returns at stage 3 below before the
    // pre_filtered join that would consume tree_lines). Mixing the two
    // doesn't make sense — tree restructures path-list output, parse
    // structures arbitrary text — but we gate the computation here so the
    // mutual exclusion is explicit and we don't pay for wasted work.
    //
    // When active, color restoration is bypassed in the pre_filtered step
    // below — color spans don't survive structural rearrangement.
    let tree_lines: Option<Vec<String>> = if config.parse.is_some() {
        None
    } else {
        config
            .tree
            .as_ref()
            .and_then(|tree_cfg| tree::apply_tree(tree_cfg, &lines))
    };

    // 2b. Lua script escape hatch (sandboxed)
    #[cfg(feature = "lua")]
    if let Some(ref script_cfg) = config.lua_script {
        let clean_text = lines.join("\n");
        if let Some(output) = run_lua(script_cfg, &clean_text, result.exit_code, args, lua_limits) {
            return FilterResult {
                output: finalize_output(config, output),
            };
        }
    }

    // 2c. JSON extraction — when configured, replaces parse/sections/chunks.
    // `has_json` = config declares [json]; `json_parsed` = input was valid JSON.
    // When parsing fails, the pipeline falls through to fallback (raw output)
    // instead of rendering templates with empty placeholders.
    let has_json = config.json.is_some();
    let (json_vars, json_chunks, json_parsed) = config.json.as_ref().map_or_else(
        || {
            (
                std::collections::HashMap::new(),
                template::ChunkMap::new(),
                false,
            )
        },
        |json_config| {
            let (parsed, vars, chunks) = json::extract_json(&result.combined, json_config);
            (vars, chunks, parsed)
        },
    );

    // 3. If parse exists → parse+output pipeline (skipped when json ran)
    if !has_json && let Some(ref parse_config) = config.parse {
        let parse_result = parse::run_parse(parse_config, &lines);
        let output_config = config.output.clone().unwrap_or_default();
        let output = parse::render_output(&output_config, &parse_result);
        return FilterResult {
            output: finalize_output(config, output),
        };
    }

    // 4. Collect sections and chunks (skipped when json ran — JSON replaces
    //    line-based structural processing).
    //    DESIGN NOTE: section enter/exit regexes match against the original,
    //    unmodified lines. If the command emits ANSI codes in marker lines,
    //    set `strip_ansi = true` AND write patterns that match the raw text,
    //    or configure the command to disable color (e.g. `--no-color`).
    let has_sections = !has_json && !config.section.is_empty();
    let needs_raw_lines = !has_json && (has_sections || !config.chunk.is_empty());
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

    // Restore display lines for color mode, join tree-rendered lines, or
    // join clean lines. The tree path takes priority over color restoration
    // because the rearranged structure can't carry per-line color spans.
    let pre_filtered = if let Some(ref t) = tree_lines {
        t.join("\n")
    } else if let Some(ref display) = raw.display {
        restore_display_lines(&raw.clean, display, &lines)
    } else {
        lines.join("\n")
    };

    let mut chunks = if !has_json && !config.chunk.is_empty() {
        chunk::process_chunks(&config.chunk, &raw_lines)
    } else {
        template::ChunkMap::new()
    };

    // Merge JSON-extracted chunks into the chunk map.
    if has_json {
        chunks.extend(json_chunks);
    }

    // 5. Select branch by exit code
    let branch = select_branch(config, result.exit_code);
    let ctx = BranchContext {
        sections: &sections,
        chunks: &chunks,
        has_sections,
        has_json,
        json_parsed,
        json_vars: &json_vars,
        top_level_tail: config.tail,
        top_level_head: config.head,
    };
    let output = branch.map_or_else(
        || apply_fallback(config, &pre_filtered),
        |b| {
            apply_branch(b, &pre_filtered, &ctx)
                .unwrap_or_else(|| apply_fallback(config, &pre_filtered))
        },
    );

    FilterResult {
        output: finalize_output(config, output),
    }
}

/// Final output processing: post-process (strip/collapse/truncate), apply
/// `max_lines` cap, then apply `on_empty`.
fn finalize_output(config: &FilterConfig, output: String) -> String {
    let mut output = cleanup::post_process_output(config, output);

    // max_lines: absolute cap applied after all other processing.
    if let Some(max) = config.max_lines {
        let trailing = output.ends_with('\n');
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() > max {
            output = lines[..max].join("\n");
            if trailing {
                output.push('\n');
            }
        }
    }

    if let Some(ref msg) = config.on_empty
        && output.trim().is_empty()
    {
        return msg.clone();
    }
    output
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
fn apply_branch(branch: &OutputBranch, combined: &str, ctx: &BranchContext<'_>) -> Option<String> {
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
        aggregate::run_aggregates(&owned_rules, ctx.sections)
    };

    // 2. Output template
    if let Some(ref output_tmpl) = branch.output {
        if ctx.has_sections {
            let any_collected = ctx
                .sections
                .values()
                .any(|s| !s.lines.is_empty() || !s.blocks.is_empty());
            if !any_collected {
                return None; // sections expected but empty → fallback
            }
        }
        // JSON configured but input wasn't valid JSON → fallback to raw output
        // instead of rendering templates with empty placeholders.
        if ctx.has_json && !ctx.json_parsed {
            return None;
        }
        let mut vars = vars;
        vars.insert("output".to_string(), combined.to_string());
        // Merge JSON-extracted vars into the template context.
        vars.extend(ctx.json_vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        return Some(template::render_template(
            output_tmpl,
            &vars,
            ctx.sections,
            ctx.chunks,
        ));
    }

    // Non-template path (tail/head/skip/extract)
    let mut lines: Vec<&str> = combined.lines().collect();

    if let Some(tail) = branch.tail.or(ctx.top_level_tail)
        && lines.len() > tail
    {
        lines = lines.split_off(lines.len() - tail);
    }
    if let Some(head) = branch.head.or(ctx.top_level_head) {
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
    let tail = config
        .fallback
        .as_ref()
        .and_then(|fb| fb.tail)
        .or(config.tail);

    if tail.is_none() && config.head.is_none() {
        return combined.to_string();
    }

    let mut lines: Vec<&str> = combined.lines().collect();
    if let Some(tail) = tail
        && lines.len() > tail
    {
        lines = lines.split_off(lines.len() - tail);
    }
    if let Some(head) = config.head {
        lines.truncate(head);
    }
    lines.join("\n")
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
mod tests_json;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_pipeline;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_rtk_compat;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_tree;
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests_tree_unit;
