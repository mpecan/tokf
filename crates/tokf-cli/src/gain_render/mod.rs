use std::fmt::Write as _;

use tokf::remote::gain_client;
use tokf::tracking::{FilterGain, GainSummary};

#[cfg(test)]
mod tests;

/// ANSI color codes (or empty strings when color is disabled).
pub struct ColorMode {
    pub reset: &'static str,
    pub bold: &'static str,
    pub dim: &'static str,
    pub green: &'static str,
    pub cyan: &'static str,
    pub yellow: &'static str,
    pub magenta: &'static str,
}

impl ColorMode {
    pub const fn new(colored: bool) -> Self {
        if colored {
            Self {
                reset: "\x1b[0m",
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                green: "\x1b[32m",
                cyan: "\x1b[36m",
                yellow: "\x1b[33m",
                magenta: "\x1b[35m",
            }
        } else {
            Self {
                reset: "",
                bold: "",
                dim: "",
                green: "",
                cyan: "",
                yellow: "",
                magenta: "",
            }
        }
    }
}

/// Returns `true` when color output should be disabled.
///
/// Color is disabled when:
/// - `no_color_flag` is `true` (the `--no-color` CLI flag), or
/// - the `NO_COLOR` environment variable is present (per <https://no-color.org/>).
pub fn should_disable_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return true;
    }
    std::env::var_os("NO_COLOR").is_some()
}

/// Convert a remote `GainResponse` into local `GainSummary` + `Vec<FilterGain>`.
/// Fields not available from the remote API (filter time, pipe overrides) are zeroed.
pub fn from_remote(resp: &gain_client::GainResponse) -> (GainSummary, Vec<FilterGain>) {
    let tokens_saved = resp.total_input_tokens - resp.total_output_tokens;
    let savings_pct = savings_pct_for(resp.total_input_tokens, tokens_saved);

    // Fallback: old servers (pre-raw_tokens) return 0 for total_raw_tokens.
    // We can't distinguish "genuine zero usage" from "server doesn't support this field",
    // but zero usage implies zero input too, so falling back to input is safe either way.
    let total_raw = if resp.total_raw_tokens > 0 {
        resp.total_raw_tokens
    } else {
        resp.total_input_tokens
    };

    let summary = GainSummary {
        total_commands: resp.total_commands,
        total_input_tokens: resp.total_input_tokens,
        total_output_tokens: resp.total_output_tokens,
        tokens_saved,
        savings_pct,
        pipe_override_count: 0,
        total_filter_time_ms: 0,
        avg_filter_time_ms: 0.0,
        total_raw_tokens: total_raw,
    };

    let filters: Vec<FilterGain> = resp
        .by_filter
        .iter()
        .map(|e| {
            let saved = e.total_input_tokens - e.total_output_tokens;
            // Same fallback as summary-level: 0 means old server or no usage.
            let raw = if e.total_raw_tokens > 0 {
                e.total_raw_tokens
            } else {
                e.total_input_tokens
            };
            FilterGain {
                filter_name: e
                    .filter_name
                    .clone()
                    .unwrap_or_else(|| "passthrough".into()),
                commands: e.total_commands,
                input_tokens: e.total_input_tokens,
                output_tokens: e.total_output_tokens,
                tokens_saved: saved,
                savings_pct: savings_pct_for(e.total_input_tokens, saved),
                pipe_override_count: 0,
                total_filter_time_ms: 0,
                avg_filter_time_ms: 0.0,
                raw_tokens: raw,
            }
        })
        .collect();

    (summary, filters)
}

#[allow(clippy::cast_precision_loss)]
fn savings_pct_for(input: i64, saved: i64) -> f64 {
    if input == 0 {
        0.0
    } else {
        saved as f64 / input as f64 * 100.0
    }
}

/// Render a percentage bar: `██████████░░░░░░░░░░` style.
///
/// `pct` is 0.0–100.0, `width` is total bar character count.
/// When `filled_color`/`empty_color`/`reset` are empty strings the output is plain.
fn render_bar(
    pct: f64,
    width: usize,
    filled_color: &str,
    empty_color: &str,
    reset: &str,
) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let mut bar = String::new();
    if filled > 0 {
        let _ = write!(bar, "{filled_color}");
        for _ in 0..filled {
            bar.push('█');
        }
        if !reset.is_empty() {
            let _ = write!(bar, "{reset}");
        }
    }
    if empty > 0 {
        let _ = write!(bar, "{empty_color}");
        for _ in 0..empty {
            bar.push('░');
        }
        if !reset.is_empty() {
            let _ = write!(bar, "{reset}");
        }
    }
    bar
}

/// Proportional bar where `value` is scaled relative to `max`.
/// Returns spaces when `max` is zero.
#[allow(clippy::too_many_arguments)]
fn render_proportional_bar(
    value: i64,
    max: i64,
    width: usize,
    filled_color: &str,
    empty_color: &str,
    reset: &str,
) -> String {
    if max <= 0 {
        return " ".repeat(width);
    }
    #[allow(clippy::cast_precision_loss)]
    let pct = (value as f64 / max as f64) * 100.0;
    render_bar(pct, width, filled_color, empty_color, reset)
}

/// Format milliseconds into human-readable duration.
/// Examples: "0.0ms", "1.9ms", "2.4s", "1m 12s"
pub fn format_time_ms(ms: f64) -> String {
    if ms < 1000.0 {
        format!("{ms:.1}ms")
    } else if ms < 60_000.0 {
        format!("{:.1}s", ms / 1000.0)
    } else {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let secs = (ms / 1000.0) as u64;
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m {s}s")
    }
}

/// Thousands-separator formatting for integers.
pub fn format_num(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let chunks: Vec<&str> = s
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let formatted = chunks.join(",");
    if n < 0 {
        format!("-{formatted}")
    } else {
        formatted
    }
}

/// Truncate a string to at most `max_bytes` bytes without panicking on
/// multi-byte character boundaries.
fn truncate_name(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Render the full graphical TTY output for `tokf gain`.
pub fn render_summary_tty(
    summary: &GainSummary,
    filters: &[FilterGain],
    top: usize,
    colors: &ColorMode,
) -> String {
    let ColorMode {
        reset,
        bold,
        dim,
        green,
        cyan,
        ..
    } = colors;
    let mut out = String::new();

    // -- Tokens section --
    let _ = writeln!(out);
    let _ = writeln!(out, "  {bold}Tokens{reset}");
    let _ = writeln!(out, "  {dim}━━━━━━{reset}");
    let _ = writeln!(
        out,
        "  {cyan}{}{reset} runs",
        format_num(summary.total_commands)
    );
    let _ = writeln!(
        out,
        "  {cyan}{}{reset} in  \u{2192}  {cyan}{}{reset} out  \u{2192}  {green}{}{reset} saved",
        format_num(summary.total_input_tokens),
        format_num(summary.total_output_tokens),
        format_num(summary.tokens_saved)
    );
    if summary.pipe_override_count > 0 {
        let _ = writeln!(
            out,
            "  ({} runs used pipe output instead)",
            summary.pipe_override_count
        );
    }

    render_raw_baseline_section(&mut out, summary, colors);

    // -- Filter Time section (only when data is available) --
    if summary.total_filter_time_ms > 0 || summary.avg_filter_time_ms > 0.0 {
        let _ = writeln!(out);
        let _ = writeln!(out, "  {bold}Filter Time{reset}");
        let _ = writeln!(out, "  {dim}━━━━━━━━━━━{reset}");
        #[allow(clippy::cast_precision_loss)]
        let total_time_display = format_time_ms(summary.total_filter_time_ms as f64);
        let avg_time_display = format_time_ms(summary.avg_filter_time_ms);
        let _ = writeln!(
            out,
            "  Total: {cyan}{total_time_display}{reset}   Avg: {cyan}{avg_time_display}{reset}/run"
        );
    }

    render_top_filters_table(&mut out, filters, top, colors);
    out
}

/// Render raw vs baseline breakdown and reduction bars.
fn render_raw_baseline_section(out: &mut String, summary: &GainSummary, colors: &ColorMode) {
    let ColorMode {
        reset,
        dim,
        green,
        cyan,
        ..
    } = colors;

    // Raw vs baseline line (only when baseline adjustment occurred)
    if summary.total_raw_tokens > summary.total_input_tokens {
        let _ = writeln!(
            out,
            "  {dim}{}{reset} intercepted \u{2192} {dim}{}{reset} baseline \u{2192} {dim}{}{reset} delivered",
            format_num(summary.total_raw_tokens),
            format_num(summary.total_input_tokens),
            format_num(summary.total_output_tokens),
        );
    }

    // Reduction bar
    let _ = writeln!(out);
    let bar = render_bar(summary.savings_pct, 50, green, dim, reset);
    let _ = writeln!(
        out,
        "  Reduction   {bar}  {green}{:.1}%{reset}",
        summary.savings_pct
    );

    // Reduction vs raw bar (only when baseline adjustment occurred)
    if summary.total_raw_tokens > summary.total_input_tokens {
        #[allow(clippy::cast_precision_loss)]
        let raw_savings_pct = if summary.total_raw_tokens == 0 {
            0.0
        } else {
            (summary.total_raw_tokens - summary.total_output_tokens) as f64
                / summary.total_raw_tokens as f64
                * 100.0
        };
        let raw_bar = render_bar(raw_savings_pct, 50, cyan, dim, reset);
        let _ = writeln!(
            out,
            "  vs raw      {raw_bar}  {cyan}{raw_savings_pct:.1}%{reset}"
        );
    }
}

/// Render the top-N filters table section for TTY output.
fn render_top_filters_table(
    out: &mut String,
    filters: &[FilterGain],
    top: usize,
    colors: &ColorMode,
) {
    let ColorMode {
        reset,
        bold,
        dim,
        green,
        yellow,
        magenta,
        ..
    } = colors;
    if filters.is_empty() {
        return;
    }
    let display_filters: Vec<&FilterGain> = filters.iter().take(top).collect();
    let max_saved = display_filters
        .iter()
        .map(|f| f.tokens_saved)
        .max()
        .unwrap_or(0);

    let _ = writeln!(out);
    let header = format!("  {bold}Top {} Filters{reset}", top.min(filters.len()));
    // Pad calculation accounts for the ANSI escape bytes in header
    let visible_len = format!("  Top {} Filters", top.min(filters.len())).len();
    let pad = 74usize.saturating_sub(visible_len);
    let _ = writeln!(out, "{header}{:>pad$}", "Saved     Pct");
    let _ = writeln!(out, "  {dim}{}{reset}", "━".repeat(72));

    for f in &display_filters {
        let name = truncate_name(&f.filter_name, 20);
        let bar = render_proportional_bar(f.tokens_saved, max_saved, 38, magenta, dim, reset);
        if f.savings_pct < 0.0 {
            let _ = writeln!(
                out,
                "  {yellow}{:<20}{reset}  {bar}  {yellow}{:>7}{reset}  {yellow}{:>4.1}%{reset} {yellow}(overhead){reset}",
                name,
                format_num(f.tokens_saved),
                f.savings_pct
            );
        } else {
            let _ = writeln!(
                out,
                "  {yellow}{:<20}{reset}  {bar}  {green}{:>7}{reset}  {green}{:>4.1}%{reset}",
                name,
                format_num(f.tokens_saved),
                f.savings_pct
            );
        }
    }
}

/// Render backward-compatible plain text for piped (non-TTY) output.
pub fn render_summary_plain(summary: &GainSummary, filters: &[FilterGain], top: usize) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "tokf gain summary");
    let _ = writeln!(out, "  total runs:     {}", summary.total_commands);
    let _ = writeln!(
        out,
        "  input tokens:   {} est.",
        format_num(summary.total_input_tokens)
    );
    let _ = writeln!(
        out,
        "  output tokens:  {} est.",
        format_num(summary.total_output_tokens)
    );
    let _ = writeln!(
        out,
        "  tokens saved:   {} est. ({:.1}%)",
        format_num(summary.tokens_saved),
        summary.savings_pct
    );
    if summary.total_raw_tokens > summary.total_input_tokens {
        let _ = writeln!(
            out,
            "  raw tokens:     {} est. (before baseline)",
            format_num(summary.total_raw_tokens)
        );
    }
    if summary.pipe_override_count > 0 {
        let _ = writeln!(
            out,
            "  pipe preferred: {} runs (pipe output was smaller than filter)",
            summary.pipe_override_count
        );
    }

    // Filter time (only when data is available)
    if summary.total_filter_time_ms > 0 || summary.avg_filter_time_ms > 0.0 {
        #[allow(clippy::cast_precision_loss)]
        let total_time = format_time_ms(summary.total_filter_time_ms as f64);
        let avg_time = format_time_ms(summary.avg_filter_time_ms);
        let _ = writeln!(
            out,
            "  filter time:    {total_time} total, {avg_time}/run avg"
        );
    }

    render_plain_filters(&mut out, filters, top);

    out
}

/// Render the top-N filters section for plain-text output.
fn render_plain_filters(out: &mut String, filters: &[FilterGain], top: usize) {
    if filters.is_empty() {
        return;
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "tokf gain by filter (top {})", top.min(filters.len()));
    for f in filters.iter().take(top) {
        let override_note = if f.pipe_override_count > 0 {
            format!("  pipe: {}", f.pipe_override_count)
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "  {:30}  runs: {:4}  saved: {} est. ({:.1}%){override_note}",
            f.filter_name,
            f.commands,
            format_num(f.tokens_saved),
            f.savings_pct
        );
    }
}
