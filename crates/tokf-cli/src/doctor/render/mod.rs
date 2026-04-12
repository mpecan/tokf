//! Human and JSON rendering for `DoctorReport`.
//!
//! The human renderer mirrors the convention used by `gain_render`:
//! ANSI codes assembled by hand from a tiny `Colors` struct, with a
//! `disabled()` constructor for `--no-color` / `NO_COLOR` env / non-TTY.
//! No external table-rendering dep.

use std::fmt::Write as _;

use super::{DoctorReport, FilterReport};

/// Minimal color palette.
///
/// Mirrors the convention in `crate::gain_render::ColorMode` but is
/// duplicated here so the doctor module stays usable from both the
/// binary and the library crate (`gain_render` is a binary-only module).
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub reset: &'static str,
    pub bold: &'static str,
    pub dim: &'static str,
    pub red: &'static str,
    pub yellow: &'static str,
    pub green: &'static str,
    pub cyan: &'static str,
}

/// ANSI-enabled palette.
pub const COLORS_ON: Colors = Colors {
    reset: "\x1b[0m",
    bold: "\x1b[1m",
    dim: "\x1b[2m",
    red: "\x1b[31m",
    yellow: "\x1b[33m",
    green: "\x1b[32m",
    cyan: "\x1b[36m",
};

/// Plain-text palette (all fields empty). Used by `--no-color`, non-TTY
/// output, and the `NO_COLOR` env var.
pub const COLORS_OFF: Colors = Colors {
    reset: "",
    bold: "",
    dim: "",
    red: "",
    yellow: "",
    green: "",
    cyan: "",
};

impl Colors {
    pub const fn enabled() -> Self {
        COLORS_ON
    }
    pub const fn disabled() -> Self {
        COLORS_OFF
    }
}

/// Returns `true` when color output should be disabled per the `--no-color`
/// flag or the `NO_COLOR` env var (<https://no-color.org/>).
pub fn should_disable_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return true;
    }
    std::env::var_os("NO_COLOR").is_some()
}

/// Pick a colour for a health score. Used in the per-filter table to make
/// "needs attention" rows visually obvious.
const fn score_color(score: u8, c: &Colors) -> &'static str {
    match score {
        0..=49 => c.red,
        50..=79 => c.yellow,
        _ => c.green,
    }
}

/// Render the report as a human-readable text block.
///
/// Layout:
///   - one-line summary header
///   - per-filter table (filter, events, score, bursts/maxBurst, flags, retries)
///   - if any bursts: top-3 burst detail block
///   - if any workaround suggestions: per-filter suggestion list
///   - if any negative-savings filters: a "filters making things worse" callout
pub fn render_human(report: &DoctorReport, colors: &Colors) -> String {
    let mut out = String::new();

    render_header(&mut out, report, colors);

    if report.total_events_considered == 0 {
        let _ = writeln!(
            out,
            "\n{dim}no events yet — run some commands first to populate tracking.db{reset}",
            dim = colors.dim,
            reset = colors.reset,
        );
        return out;
    }

    if report.filters.is_empty() {
        let _ = writeln!(
            out,
            "\n{dim}no filtered events match the current scope{reset}",
            dim = colors.dim,
            reset = colors.reset,
        );
        return out;
    }

    render_filter_table(&mut out, &report.filters, colors);

    if !report.bursts.is_empty() {
        render_burst_detail(&mut out, report, colors);
    }

    let any_suggestions = report
        .filters
        .iter()
        .any(|f| !f.untracked_workaround_flags.is_empty());
    if any_suggestions {
        render_workaround_suggestions(&mut out, &report.filters, colors);
    }

    let any_negative = report
        .filters
        .iter()
        .any(|f| f.avg_excess_tokens.is_some_and(|v| v > 0.0));
    if any_negative {
        render_negative_savings(&mut out, &report.filters, colors);
    }

    out
}

fn render_header(out: &mut String, report: &DoctorReport, c: &Colors) {
    let scope = report
        .project_filter
        .as_deref()
        .map_or_else(|| "all projects".to_string(), |p| format!("project={p}"));
    let _ = writeln!(
        out,
        "{bold}tokf doctor{reset} — {dim}{events} events, {scope}, threshold≥{th} within {win}s{reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
        events = report.total_events_considered,
        scope = scope,
        th = report.burst_threshold,
        win = report.window_secs,
    );
}

fn render_filter_table(out: &mut String, filters: &[FilterReport], c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}{:<22} {:>7} {:>6} {:>9} {:>9} {:>9}{reset}",
        "filter",
        "events",
        "score",
        "bursts",
        "max-burst",
        "retries",
        bold = c.bold,
        reset = c.reset,
    );
    let _ = writeln!(out, "{}", "─".repeat(70));
    for f in filters {
        let score_col = score_color(f.health_score, c);
        let max_burst = if f.max_burst_size == 0 {
            "-".to_string()
        } else {
            f.max_burst_size.to_string()
        };
        let retries = if f.empty_retry_count == 0 {
            "-".to_string()
        } else {
            f.empty_retry_count.to_string()
        };
        let _ = writeln!(
            out,
            "{:<22} {:>7} {col}{:>6}{reset} {:>9} {:>9} {:>9}",
            truncate(&f.filter_name, 22),
            f.event_count,
            f.health_score,
            f.burst_count,
            max_burst,
            retries,
            col = score_col,
            reset = c.reset,
        );
    }
}

fn render_burst_detail(out: &mut String, report: &DoctorReport, c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}retry-burst detail{reset} {dim}(top 5 by size){reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
    );
    for b in report.bursts.iter().take(5) {
        let _ = writeln!(
            out,
            "  {yellow}×{}{reset} {} {dim}({}){reset}",
            b.burst_size,
            super::noise::command_shape(&b.command),
            b.filter_name,
            yellow = c.yellow,
            reset = c.reset,
            dim = c.dim,
        );
    }
}

fn render_workaround_suggestions(out: &mut String, filters: &[FilterReport], c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}workaround-flag suggestions{reset} {dim}(consider adding to passthrough_args){reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
    );
    for f in filters {
        if f.untracked_workaround_flags.is_empty() {
            continue;
        }
        let flags: Vec<String> = f
            .untracked_workaround_flags
            .iter()
            .map(|w| format!("{}×{}", w.flag, w.count))
            .collect();
        let _ = writeln!(
            out,
            "  {cyan}{}{reset}: {}",
            f.filter_name,
            flags.join(", "),
            cyan = c.cyan,
            reset = c.reset
        );
    }
}

fn render_negative_savings(out: &mut String, filters: &[FilterReport], c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}filters with negative token savings{reset} {dim}(filtered output > raw){reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
    );
    for f in filters {
        let Some(excess) = f.avg_excess_tokens else {
            continue;
        };
        if excess <= 0.0 {
            continue;
        }
        let _ = writeln!(
            out,
            "  {red}+{:.1}{reset} avg tokens per call — {}",
            excess,
            f.filter_name,
            red = c.red,
            reset = c.reset,
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests;
