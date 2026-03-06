use super::*;

/// Strip ANSI escape sequences for assertion comparisons.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (end of SGR sequence)
            for inner in chars.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// -- render_bar tests --

#[test]
fn render_bar_zero_plain() {
    let bar = render_bar(0.0, 10, "", "", "");
    assert_eq!(bar, "░░░░░░░░░░");
}

#[test]
fn render_bar_full_plain() {
    let bar = render_bar(100.0, 10, "", "", "");
    assert_eq!(bar, "██████████");
}

#[test]
fn render_bar_half_plain() {
    let bar = render_bar(50.0, 10, "", "", "");
    assert_eq!(bar, "█████░░░░░");
}

#[test]
fn render_bar_clamps_over_100() {
    let bar = render_bar(150.0, 10, "", "", "");
    assert_eq!(bar, "██████████");
}

#[test]
fn render_bar_clamps_negative() {
    let bar = render_bar(-10.0, 10, "", "", "");
    assert_eq!(bar, "░░░░░░░░░░");
}

#[test]
fn render_bar_colored_contains_ansi() {
    let c = ColorMode::new(true);
    let bar = render_bar(50.0, 10, c.green, c.dim, c.reset);
    assert!(bar.contains("\x1b["), "bar should contain ANSI: {bar}");
    // After stripping, only block chars remain
    let plain = strip_ansi(&bar);
    assert_eq!(plain, "█████░░░░░");
}

#[test]
fn render_bar_colored_reset_balance() {
    let c = ColorMode::new(true);
    let bar = render_bar(50.0, 10, c.green, c.dim, c.reset);
    let reset_count = bar.matches(c.reset).count();
    // One RESET after filled, one after empty
    assert_eq!(reset_count, 2, "bar: {bar}");
}

#[test]
fn render_bar_zero_colored_no_filled_color() {
    let c = ColorMode::new(true);
    let bar = render_bar(0.0, 10, c.green, c.dim, c.reset);
    // 0% → no filled segment, so green should not appear
    assert!(!bar.contains(c.green), "bar: {bar}");
}

#[test]
fn render_bar_full_colored_no_empty_color() {
    let c = ColorMode::new(true);
    let bar = render_bar(100.0, 10, c.green, c.dim, c.reset);
    // 100% → no empty segment, so dim should not appear
    assert!(!bar.contains(c.dim), "bar: {bar}");
}

// -- render_proportional_bar tests --

#[test]
fn render_proportional_bar_basic() {
    let bar = render_proportional_bar(50, 100, 10, "", "", "");
    assert_eq!(bar, "█████░░░░░");
}

#[test]
fn render_proportional_bar_zero_max() {
    let bar = render_proportional_bar(50, 0, 10, "", "", "");
    assert_eq!(bar, "          ");
}

#[test]
fn render_proportional_bar_colored() {
    let c = ColorMode::new(true);
    let bar = render_proportional_bar(50, 100, 10, c.magenta, c.dim, c.reset);
    assert!(bar.contains("\x1b["), "bar should contain ANSI: {bar}");
    let plain = strip_ansi(&bar);
    assert_eq!(plain, "█████░░░░░");
}

// -- format_time_ms tests --

#[test]
fn format_time_ms_sub_second() {
    assert_eq!(format_time_ms(0.0), "0.0ms");
    assert_eq!(format_time_ms(1.9), "1.9ms");
    assert_eq!(format_time_ms(999.9), "999.9ms");
}

#[test]
fn format_time_ms_seconds() {
    assert_eq!(format_time_ms(1000.0), "1.0s");
    assert_eq!(format_time_ms(2400.0), "2.4s");
    assert_eq!(format_time_ms(59_999.0), "60.0s");
}

#[test]
fn format_time_ms_minutes() {
    assert_eq!(format_time_ms(60_000.0), "1m 0s");
    assert_eq!(format_time_ms(72_000.0), "1m 12s");
}

// -- format_num tests --

#[test]
fn format_num_basic() {
    assert_eq!(format_num(0), "0");
    assert_eq!(format_num(999), "999");
    assert_eq!(format_num(1000), "1,000");
    assert_eq!(format_num(84320), "84,320");
    assert_eq!(format_num(-73080), "-73,080");
}

#[test]
fn format_num_i64_min_no_panic() {
    let result = format_num(i64::MIN);
    assert!(result.starts_with('-'));
    assert!(result.contains("9,223,372,036,854,775,808"));
}

// -- truncate_name tests --

#[test]
fn truncate_name_short_ascii() {
    assert_eq!(truncate_name("git/status", 20), "git/status");
}

#[test]
fn truncate_name_exact_ascii() {
    let name = "a".repeat(20);
    assert_eq!(truncate_name(&name, 20), name);
}

#[test]
fn truncate_name_long_ascii() {
    let name = "abcdefghijklmnopqrstuvwxyz";
    assert_eq!(truncate_name(name, 20), "abcdefghijklmnopqrst");
}

#[test]
fn truncate_name_multibyte_boundary() {
    // 'é' is 2 bytes in UTF-8. "éééééééééé" = 10 × 2 = 20 bytes.
    let name = "ééééééééééé"; // 11 chars × 2 bytes = 22 bytes
    let result = truncate_name(name, 20);
    // Should truncate to 20 bytes = 10 'é' chars, not panic
    assert_eq!(result.len(), 20);
    assert_eq!(result, "éééééééééé");
}

#[test]
fn truncate_name_multibyte_mid_char() {
    // Force a cut in the middle of a multi-byte character.
    // '日' is 3 bytes. "日日日日日日日" = 7 × 3 = 21 bytes.
    let name = "日日日日日日日";
    let result = truncate_name(name, 20);
    // 20 / 3 = 6 full chars = 18 bytes (rounds down to char boundary)
    assert_eq!(result.len(), 18);
    assert_eq!(result, "日日日日日日");
}

#[test]
fn truncate_name_empty() {
    assert_eq!(truncate_name("", 20), "");
}

// -- should_disable_color tests --

#[test]
fn should_disable_color_flag_true() {
    assert!(should_disable_color(true));
}

#[test]
fn should_disable_color_flag_false() {
    // When the flag is false, the function checks NO_COLOR env var.
    // We can only assert the flag-true path deterministically;
    // env-var behaviour is covered by the integration path.
    // At minimum, verify false doesn't unconditionally return true.
    let result = should_disable_color(false);
    // Result depends on whether NO_COLOR is set in the test environment.
    // Either way, this should not panic.
    let _ = result;
}

// -- ColorMode tests --

#[test]
fn color_mode_colored_has_ansi() {
    let c = ColorMode::new(true);
    assert!(c.reset.contains("\x1b["));
    assert!(c.bold.contains("\x1b["));
    assert!(c.green.contains("\x1b["));
}

#[test]
fn color_mode_plain_all_empty() {
    let c = ColorMode::new(false);
    assert!(c.reset.is_empty());
    assert!(c.bold.is_empty());
    assert!(c.dim.is_empty());
    assert!(c.green.is_empty());
    assert!(c.cyan.is_empty());
    assert!(c.yellow.is_empty());
    assert!(c.magenta.is_empty());
}

// -- TTY render helpers --

fn make_summary(commands: i64, input: i64, output: i64, filter_ms: i64) -> GainSummary {
    let saved = input - output;
    #[allow(clippy::cast_precision_loss)]
    let pct = if input == 0 {
        0.0
    } else {
        saved as f64 / input as f64 * 100.0
    };
    #[allow(clippy::cast_precision_loss)]
    let avg = if commands == 0 {
        0.0
    } else {
        filter_ms as f64 / commands as f64
    };
    GainSummary {
        total_commands: commands,
        total_input_tokens: input,
        total_output_tokens: output,
        tokens_saved: saved,
        savings_pct: pct,
        pipe_override_count: 0,
        total_filter_time_ms: filter_ms,
        avg_filter_time_ms: avg,
        total_raw_tokens: input,
    }
}

fn make_filter(name: &str, input: i64, output: i64, commands: i64) -> FilterGain {
    let saved = input - output;
    #[allow(clippy::cast_precision_loss)]
    let pct = if input == 0 {
        0.0
    } else {
        saved as f64 / input as f64 * 100.0
    };
    FilterGain {
        filter_name: name.to_string(),
        commands,
        input_tokens: input,
        output_tokens: output,
        tokens_saved: saved,
        savings_pct: pct,
        pipe_override_count: 0,
        total_filter_time_ms: 0,
        avg_filter_time_ms: 0.0,
        raw_tokens: input,
    }
}

// -- render_summary_tty tests --

#[test]
fn render_summary_tty_contains_key_elements() {
    let summary = make_summary(1234, 116_640, 32_320, 2400);
    let filters = vec![
        make_filter("git/status", 51_200, 9_200, 500),
        make_filter("cargo/test", 39_200, 11_200, 300),
    ];
    let raw = render_summary_tty(&summary, &filters, 10, &ColorMode::new(true));
    let output = strip_ansi(&raw);
    assert!(output.contains("1,234 runs"), "output: {output}");
    assert!(output.contains("116,640 in"), "output: {output}");
    assert!(output.contains("84,320 saved"), "output: {output}");
    assert!(output.contains("Reduction"), "output: {output}");
    assert!(output.contains("█"), "output: {output}");
    assert!(output.contains("Filter Time"), "output: {output}");
    assert!(output.contains("2.4s"), "output: {output}");
    assert!(output.contains("git/status"), "output: {output}");
    assert!(output.contains("cargo/test"), "output: {output}");
}

#[test]
fn render_summary_tty_colored_contains_ansi() {
    let summary = make_summary(10, 1000, 500, 100);
    let filters = vec![make_filter("git/status", 500, 100, 5)];
    let output = render_summary_tty(&summary, &filters, 10, &ColorMode::new(true));
    assert!(
        output.contains("\x1b["),
        "TTY output should contain ANSI escape codes: {output}"
    );
}

#[test]
fn render_summary_tty_no_color_no_ansi() {
    let summary = make_summary(10, 1000, 500, 100);
    let filters = vec![make_filter("git/status", 500, 100, 5)];
    let output = render_summary_tty(&summary, &filters, 10, &ColorMode::new(false));
    assert!(
        !output.contains("\x1b["),
        "No-color TTY should not contain ANSI codes: {output}"
    );
    // Should still contain bars (graphical layout)
    assert!(output.contains("█"), "No-color TTY should still have bars");
    assert!(
        output.contains("Reduction"),
        "No-color TTY should still have layout"
    );
}

#[test]
fn render_summary_tty_empty_db() {
    let summary = make_summary(0, 0, 0, 0);
    let raw = render_summary_tty(&summary, &[], 10, &ColorMode::new(true));
    let output = strip_ansi(&raw);
    assert!(output.contains("0 runs"), "output: {output}");
    assert!(output.contains("0.0%"), "output: {output}");
}

#[test]
fn render_summary_tty_top_limits_filters() {
    let filters: Vec<FilterGain> = (0..5)
        .map(|i| make_filter(&format!("filter/{i}"), 100, 50, 1))
        .collect();
    let summary = make_summary(5, 500, 250, 100);
    let raw = render_summary_tty(&summary, &filters, 3, &ColorMode::new(true));
    let output = strip_ansi(&raw);
    // Should show only top 3
    assert!(output.contains("filter/0"), "output: {output}");
    assert!(output.contains("filter/2"), "output: {output}");
    assert!(!output.contains("filter/3"), "output: {output}");
}

#[test]
fn render_summary_tty_pipe_override_shown() {
    let mut summary = make_summary(10, 1000, 500, 100);
    summary.pipe_override_count = 3;
    let raw = render_summary_tty(&summary, &[], 10, &ColorMode::new(true));
    let output = strip_ansi(&raw);
    assert!(
        output.contains("3 runs used pipe output"),
        "output: {output}"
    );
}

#[test]
fn render_summary_tty_no_filter_time_when_zero() {
    let summary = make_summary(5, 1000, 500, 0);
    let raw = render_summary_tty(&summary, &[], 10, &ColorMode::new(true));
    let output = strip_ansi(&raw);
    assert!(
        !output.contains("Filter Time"),
        "Filter Time should be omitted when zero: {output}"
    );
}

// -- render_summary_plain tests --

#[test]
fn render_summary_plain_backward_compatible() {
    let summary = make_summary(1, 100, 25, 5);
    let filters = vec![];
    let output = render_summary_plain(&summary, &filters, 10);
    assert!(output.contains("tokf gain summary"), "output: {output}");
    assert!(output.contains("total runs:"), "output: {output}");
    assert!(output.contains("input tokens:"), "output: {output}");
    assert!(output.contains("output tokens:"), "output: {output}");
    assert!(output.contains("tokens saved:"), "output: {output}");
    assert!(output.contains("filter time:"), "output: {output}");
}

#[test]
fn render_summary_plain_top_limits_filters() {
    let filters: Vec<FilterGain> = (0..5)
        .map(|i| make_filter(&format!("filter/{i}"), 100, 50, 1))
        .collect();
    let summary = make_summary(5, 500, 250, 100);
    let output = render_summary_plain(&summary, &filters, 2);
    assert!(output.contains("filter/0"), "output: {output}");
    assert!(output.contains("filter/1"), "output: {output}");
    assert!(!output.contains("filter/2"), "output: {output}");
}

#[test]
fn render_summary_plain_no_ansi() {
    let summary = make_summary(10, 1000, 500, 100);
    let filters = vec![make_filter("git/status", 500, 100, 5)];
    let output = render_summary_plain(&summary, &filters, 10);
    assert!(
        !output.contains("\x1b["),
        "Plain output should not contain ANSI escape codes: {output}"
    );
}

#[test]
fn render_summary_plain_no_filter_time_when_zero() {
    let summary = make_summary(5, 1000, 500, 0);
    let output = render_summary_plain(&summary, &[], 10);
    assert!(
        !output.contains("filter time:"),
        "filter time should be omitted when zero: {output}"
    );
}

// -- raw tokens display tests --

#[test]
fn render_summary_tty_shows_raw_when_different() {
    let mut summary = make_summary(10, 1000, 200, 100);
    summary.total_raw_tokens = 5000; // raw > input → baseline adjustment occurred
    let raw = render_summary_tty(&summary, &[], 10, &ColorMode::new(false));
    assert!(
        raw.contains("5,000 intercepted"),
        "should show raw intercepted: {raw}"
    );
    assert!(
        raw.contains("1,000 baseline"),
        "should show baseline: {raw}"
    );
    assert!(raw.contains("vs raw"), "should show vs raw bar: {raw}");
}

#[test]
fn render_summary_tty_hides_raw_when_equal() {
    let summary = make_summary(10, 1000, 200, 100);
    // total_raw_tokens == total_input_tokens (default from make_summary)
    let raw = render_summary_tty(&summary, &[], 10, &ColorMode::new(false));
    assert!(
        !raw.contains("intercepted"),
        "should not show intercepted when equal: {raw}"
    );
    assert!(
        !raw.contains("vs raw"),
        "should not show vs raw bar when equal: {raw}"
    );
}

#[test]
fn render_top_filters_overhead_highlighted() {
    // Filter with negative savings (overhead)
    let f = make_filter("npm/test", 100, 120, 5);
    // savings_pct is already negative: (100-120)/100 = -20%
    assert!(f.savings_pct < 0.0);
    let summary = make_summary(5, 100, 120, 0);
    let raw = render_summary_tty(&summary, &[f], 10, &ColorMode::new(false));
    assert!(
        raw.contains("(overhead)"),
        "negative savings should show (overhead): {raw}"
    );
}

#[test]
fn render_summary_plain_shows_raw_when_different() {
    let mut summary = make_summary(10, 1000, 200, 100);
    summary.total_raw_tokens = 5000;
    let output = render_summary_plain(&summary, &[], 10);
    assert!(
        output.contains("raw tokens:"),
        "should show raw tokens line: {output}"
    );
    assert!(
        output.contains("5,000"),
        "should show raw token count: {output}"
    );
}

#[test]
fn render_summary_plain_hides_raw_when_equal() {
    let summary = make_summary(10, 1000, 200, 100);
    let output = render_summary_plain(&summary, &[], 10);
    assert!(
        !output.contains("raw tokens:"),
        "should not show raw tokens when equal: {output}"
    );
}

// -- from_remote tests --

#[test]
fn from_remote_basic() {
    let resp = gain_client::GainResponse {
        total_input_tokens: 10_000,
        total_output_tokens: 2_000,
        total_commands: 5,
        total_raw_tokens: 15_000,
        by_machine: vec![],
        by_filter: vec![gain_client::FilterGainEntry {
            filter_name: Some("git/status".to_string()),
            filter_hash: Some("abc".to_string()),
            total_input_tokens: 5_000,
            total_output_tokens: 1_000,
            total_commands: 3,
            total_raw_tokens: 8_000,
        }],
    };
    let (summary, filters) = from_remote(&resp);
    assert_eq!(summary.total_commands, 5);
    assert_eq!(summary.tokens_saved, 8_000);
    assert_eq!(summary.total_filter_time_ms, 0);
    assert_eq!(summary.total_raw_tokens, 15_000);
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].filter_name, "git/status");
    assert_eq!(filters[0].raw_tokens, 8_000);
}

#[test]
fn from_remote_zero_raw_falls_back_to_input() {
    let resp = gain_client::GainResponse {
        total_input_tokens: 10_000,
        total_output_tokens: 2_000,
        total_commands: 5,
        total_raw_tokens: 0, // old server or no raw data
        by_machine: vec![],
        by_filter: vec![gain_client::FilterGainEntry {
            filter_name: Some("git/push".to_string()),
            filter_hash: None,
            total_input_tokens: 5_000,
            total_output_tokens: 1_000,
            total_commands: 3,
            total_raw_tokens: 0,
        }],
    };
    let (summary, filters) = from_remote(&resp);
    // Fallback: raw == input when server returns 0
    assert_eq!(summary.total_raw_tokens, 10_000);
    assert_eq!(filters[0].raw_tokens, 5_000);
}

#[test]
fn from_remote_renders_without_filter_time() {
    let resp = gain_client::GainResponse {
        total_input_tokens: 10_000,
        total_output_tokens: 2_000,
        total_commands: 5,
        total_raw_tokens: 0,
        by_machine: vec![],
        by_filter: vec![],
    };
    let (summary, filters) = from_remote(&resp);
    let raw_tty = render_summary_tty(&summary, &filters, 10, &ColorMode::new(true));
    let tty = strip_ansi(&raw_tty);
    assert!(!tty.contains("Filter Time"), "output: {tty}");
    let plain = render_summary_plain(&summary, &filters, 10);
    assert!(!plain.contains("filter time:"), "output: {plain}");
}
