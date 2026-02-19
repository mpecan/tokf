mod aggregate;
mod extract;
mod group;
mod lua;
mod match_output;
mod parse;
pub mod section;
mod skip;
mod template;

use crate::config::types::{FilterConfig, OutputBranch};
use crate::runner::CommandResult;

use self::section::SectionMap;

/// The result of applying a filter to command output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterResult {
    pub output: String,
}

/// Apply a filter configuration to a command result.
///
/// Processing order:
/// 1. `match_output` — substring check against combined output, first match wins
/// 2. Top-level `skip`/`keep` pre-filtering
///    2b. Lua script escape hatch (if configured)
/// 3. If `parse` exists → parse+output (alternative path to branches)
/// 4. Collect sections (state machine routing)
/// 5. Select branch by exit code (0 → `on_success`, else → `on_failure`)
/// 6. Apply branch with sections, or fallback
pub fn apply(config: &FilterConfig, result: &CommandResult, args: &[String]) -> FilterResult {
    // 1. match_output short-circuit
    if let Some(rule) = match_output::find_matching_rule(&config.match_output, &result.combined) {
        let output = match_output::render_output(&rule.output, &rule.contains, &result.combined);
        return FilterResult { output };
    }

    // 2. Top-level skip/keep pre-filtering
    let lines: Vec<&str> = result.combined.lines().collect();
    let lines = skip::apply_skip(&config.skip, &lines);
    let lines = skip::apply_keep(&config.keep, &lines);

    // 2b. Lua script escape hatch
    if let Some(ref script_cfg) = config.lua_script {
        let pre_filtered = lines.join("\n");
        match lua::run_lua_script(script_cfg, &pre_filtered, result.exit_code, args) {
            Ok(Some(output)) => return FilterResult { output },
            Ok(None) => {} // passthrough → continue normal pipeline
            Err(e) => eprintln!("[tokf] lua script error: {e:#}"),
        }
    }

    // 3. If parse exists → parse+output pipeline
    if let Some(ref parse_config) = config.parse {
        let parse_result = parse::run_parse(parse_config, &lines);
        let output_config = config.output.clone().unwrap_or_default();
        let output = parse::render_output(&output_config, &parse_result);
        return FilterResult { output };
    }

    // 4. Collect sections (from raw output — sections need structural
    //    markers like blank lines that skip patterns remove)
    let has_sections = !config.section.is_empty();
    let sections = if has_sections {
        let raw_lines: Vec<&str> = result.combined.lines().collect();
        section::collect_sections(&config.section, &raw_lines)
    } else {
        SectionMap::new()
    };

    // 5. Select branch by exit code
    let branch = select_branch(config, result.exit_code);

    // 6. Apply branch with sections, or fallback
    let pre_filtered = lines.join("\n");
    let output = branch.map_or_else(
        || apply_fallback(config, &pre_filtered),
        |b| {
            apply_branch(b, &pre_filtered, &sections, has_sections)
                .unwrap_or_else(|| apply_fallback(config, &pre_filtered))
        },
    );

    FilterResult { output }
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
    has_sections: bool,
) -> Option<String> {
    // 1. Aggregation
    let vars = branch
        .aggregate
        .as_ref()
        .map_or_else(std::collections::HashMap::new, |agg_rule| {
            aggregate::run_aggregate(agg_rule, sections)
        });

    // 2. Output template
    if let Some(ref output_tmpl) = branch.output {
        if has_sections {
            let any_collected = sections
                .values()
                .any(|s| !s.lines.is_empty() || !s.blocks.is_empty());
            if !any_collected && vars.is_empty() {
                return None; // sections expected but empty → fallback
            }
        }
        let mut vars = vars;
        vars.insert("output".to_string(), combined.to_string());
        return Some(template::render_template(output_tmpl, &vars, sections));
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
