//! Human and JSON rendering for `DoctorReport`.

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

pub const COLORS_ON: Colors = Colors {
    reset: "\x1b[0m",
    bold: "\x1b[1m",
    dim: "\x1b[2m",
    red: "\x1b[31m",
    yellow: "\x1b[33m",
    green: "\x1b[32m",
    cyan: "\x1b[36m",
};

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

pub fn should_disable_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return true;
    }
    std::env::var_os("NO_COLOR").is_some()
}

const fn score_color(score: u8, c: &Colors) -> &'static str {
    match score {
        0..=49 => c.red,
        50..=79 => c.yellow,
        _ => c.green,
    }
}

/// Render the report as a human-readable text block.
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

    if !report.shape_bursts.is_empty() {
        render_shape_burst_detail(&mut out, report, colors);
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
        "{bold}tokf doctor{reset} — {dim}{events} events, {scope}, \
         threshold≥{th} within {win}s{reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
        events = report.total_events_considered,
        th = report.burst_threshold,
        win = report.window_secs,
    );
}

fn render_filter_table(out: &mut String, filters: &[FilterReport], c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}{:<22} {:>6} {:>5} {:>7} {:>5} {:>5} {:>5} {:>5}{reset}",
        "filter",
        "events",
        "score",
        "bursts",
        "fail%",
        "uniq",
        "chain",
        "pipe%",
        bold = c.bold,
        reset = c.reset,
    );
    let _ = writeln!(out, "{}", "─".repeat(72));
    for f in filters {
        let col = score_color(f.health_score, c);
        let fail_pct = if f.burst_count == 0 {
            "-".to_string()
        } else {
            format!("{:.0}%", f.failed_burst_ratio * 100.0)
        };
        let uniq = f
            .median_arg_uniqueness
            .map_or_else(|| "-".to_string(), |v| format!("{v:.2}"));
        let chain = if f.max_empty_chain == 0 {
            "-".to_string()
        } else {
            f.max_empty_chain.to_string()
        };
        let pipe = if f.pipe_override_rate < 0.005 {
            "-".to_string()
        } else {
            format!("{:.0}%", f.pipe_override_rate * 100.0)
        };
        let _ = writeln!(
            out,
            "{:<22} {:>6} {col}{:>5}{reset} {:>7} {:>5} {:>5} {:>5} {:>5}",
            truncate(&f.filter_name, 22),
            f.event_count,
            f.health_score,
            f.burst_count,
            fail_pct,
            uniq,
            chain,
            pipe,
            col = col,
            reset = c.reset,
        );
    }
}

fn render_burst_detail(out: &mut String, report: &DoctorReport, c: &Colors) {
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}retry-burst detail{reset} {dim}(top 5 exact-match by size){reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
    );
    for b in report.bursts.iter().take(5) {
        let fail_note = if b.failed_count > 0 {
            format!(
                " {red}{} failed{reset}",
                b.failed_count,
                red = c.red,
                reset = c.reset
            )
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "  {yellow}×{}{reset} {} {dim}({}){reset}{fail_note}",
            b.burst_size,
            super::noise::command_shape(&b.command),
            b.filter_name,
            yellow = c.yellow,
            reset = c.reset,
            dim = c.dim,
        );
    }
}

fn render_shape_burst_detail(out: &mut String, report: &DoctorReport, c: &Colors) {
    // Only show shape bursts that have arg_uniqueness > 0.2 — those are
    // the flag-cycling pattern. Exact-match bursts already cover the
    // pure-confusion case (uniqueness ≈ 0).
    let cycling: Vec<_> = report
        .shape_bursts
        .iter()
        .filter(|b| b.arg_uniqueness > 0.2)
        .take(5)
        .collect();
    if cycling.is_empty() {
        return;
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{bold}flag-cycling bursts{reset} {dim}(shape-grouped, top 5 by size){reset}",
        bold = c.bold,
        reset = c.reset,
        dim = c.dim,
    );
    for b in &cycling {
        let _ = writeln!(
            out,
            "  {yellow}×{}{reset} {} {dim}({}, {}/{} unique, uniq={:.2}){reset}",
            b.burst_size,
            b.shape,
            b.filter_name,
            b.distinct_commands,
            b.burst_size,
            b.arg_uniqueness,
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
        "{bold}workaround-flag suggestions{reset} \
         {dim}(consider adding to passthrough_args){reset}",
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
        "{bold}filters with negative token savings{reset} \
         {dim}(filtered output > raw){reset}",
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
