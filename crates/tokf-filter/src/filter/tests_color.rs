use super::*;
use crate::CommandResult;

fn make_result(combined: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined: combined.to_string(),
    }
}

fn color_opts() -> FilterOptions {
    FilterOptions {
        preserve_color: true,
    }
}

// --- color passthrough tests ---

#[test]
fn apply_color_flag_strips_for_matching_preserves_output() {
    // skip pattern matches clean text, but surviving lines retain ANSI colors
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^warning"]
"#,
    )
    .unwrap();
    let result = make_result(
        "\x1b[33mwarning\x1b[0m: overflow\n\x1b[32minfo: ok\x1b[0m",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "\x1b[32minfo: ok\x1b[0m");
}

#[test]
fn apply_color_flag_with_dedup() {
    // Two lines identical after ANSI stripping but with different color codes.
    // Dedup collapses them; output retains the first occurrence's colors.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
dedup = true
"#,
    )
    .unwrap();
    let result = make_result(
        "\x1b[31mdup\x1b[0m\n\x1b[32mdup\x1b[0m\n\x1b[34munique\x1b[0m",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "\x1b[31mdup\x1b[0m\n\x1b[34munique\x1b[0m");
}

#[test]
fn apply_color_flag_false_unchanged() {
    // color=false produces the same output as current behavior (regression test)
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^warning"]
"#,
    )
    .unwrap();
    let result = make_result("warning: overflow\ninfo: ok", 0);
    let with_color = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(with_color.output, "info: ok");
}

#[test]
fn apply_color_flag_with_strip_ansi_true() {
    // Filter has strip_ansi = true AND color = true: global --color overrides,
    // colors are still preserved in output.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_ansi = true
skip = ["^warning"]
"#,
    )
    .unwrap();
    let result = make_result(
        "\x1b[33mwarning\x1b[0m: overflow\n\x1b[32minfo: ok\x1b[0m",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "\x1b[32minfo: ok\x1b[0m");
}

#[test]
fn apply_color_flag_no_ansi_in_input() {
    // color=true with plain text input: no change in output
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^noise"]
"#,
    )
    .unwrap();
    let result = make_result("noise line\nkeep this", 0);
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "keep this");
}

#[test]
fn apply_color_with_keep() {
    // keep pattern matches clean text, surviving lines retain ANSI colors
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
keep = ["^info"]
"#,
    )
    .unwrap();
    let result = make_result(
        "\x1b[33mwarning: bad\x1b[0m\n\x1b[32minfo: ok\x1b[0m\n\x1b[31merror: fail\x1b[0m",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "\x1b[32minfo: ok\x1b[0m");
}

#[test]
fn apply_color_with_branch_template() {
    // {output} in a branch template should contain colored text
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^noise"]
[on_success]
output = "Result:\n{output}"
"#,
    )
    .unwrap();
    let result = make_result("noise line\n\x1b[32mok: done\x1b[0m", 0);
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "Result:\n\x1b[32mok: done\x1b[0m");
}

#[test]
fn apply_color_with_trim_lines() {
    // trim_lines affects clean lines (for matching) but display lines keep
    // original whitespace including ANSI codes
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
trim_lines = true
keep = ["^OK"]
"#,
    )
    .unwrap();
    let result = make_result(
        "  \x1b[32m  OK done  \x1b[0m  \n  \x1b[31m  FAIL  \x1b[0m  ",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    // Display line is NOT trimmed — original whitespace + ANSI preserved
    assert_eq!(filtered.output, "  \x1b[32m  OK done  \x1b[0m  ");
}

#[test]
fn apply_color_all_lines_skipped() {
    // All lines match skip → empty output
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = [".*"]
"#,
    )
    .unwrap();
    let result = make_result("\x1b[31mline1\x1b[0m\n\x1b[32mline2\x1b[0m", 0);
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "");
}

#[test]
fn apply_color_empty_input() {
    let config: FilterConfig = toml::from_str(r#"command = "test""#).unwrap();
    let result = make_result("", 0);
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "");
}

#[test]
fn apply_color_with_replace() {
    // [[replace]] runs before the color split. The replace pattern operates on
    // the raw line (including ANSI codes). Both display and clean lines are
    // derived from the post-replace result.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^noise"]
[[replace]]
pattern = "^prefix: (.+)$"
output = "{1}"
"#,
    )
    .unwrap();
    // Line 1: replace extracts the colored "hello", which becomes both display and clean.
    // Line 2: "noise..." is not matched by replace, then skip removes it.
    let result = make_result("prefix: \x1b[31mhello\x1b[0m\nnoise: drop me", 0);
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(filtered.output, "\x1b[31mhello\x1b[0m");
}

#[test]
fn apply_color_with_fallback_tail() {
    // fallback tail truncation preserves ANSI codes on surviving lines
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
[fallback]
tail = 2
"#,
    )
    .unwrap();
    let result = make_result(
        "\x1b[31mline1\x1b[0m\n\x1b[32mline2\x1b[0m\n\x1b[33mline3\x1b[0m\n\x1b[34mline4\x1b[0m",
        0,
    );
    let filtered = apply(&config, &result, &[], &color_opts());
    assert_eq!(
        filtered.output,
        "\x1b[33mline3\x1b[0m\n\x1b[34mline4\x1b[0m"
    );
}

// --- restore_display_lines unit tests ---

#[test]
fn restore_display_basic_mapping() {
    // 5 clean lines, skip removes indices 1 and 3
    let clean: Vec<String> = vec!["a", "b", "c", "d", "e"]
        .into_iter()
        .map(String::from)
        .collect();
    let display: Vec<String> = vec![
        "\x1b[31ma\x1b[0m",
        "\x1b[32mb\x1b[0m",
        "\x1b[33mc\x1b[0m",
        "\x1b[34md\x1b[0m",
        "\x1b[35me\x1b[0m",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    // survivors: indices 0, 2, 4
    let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
    let survivors = vec![refs[0], refs[2], refs[4]];
    let result = restore_display_lines(&clean, &display, &survivors);
    assert_eq!(
        result,
        "\x1b[31ma\x1b[0m\n\x1b[33mc\x1b[0m\n\x1b[35me\x1b[0m"
    );
}

#[test]
fn restore_display_empty_survivors() {
    let clean: Vec<String> = vec!["a", "b"].into_iter().map(String::from).collect();
    let display: Vec<String> = vec!["A", "B"].into_iter().map(String::from).collect();
    let result = restore_display_lines(&clean, &display, &[]);
    assert_eq!(result, "");
}

#[test]
fn restore_display_all_survive() {
    let clean: Vec<String> = vec!["x", "y", "z"].into_iter().map(String::from).collect();
    let display: Vec<String> = vec!["X", "Y", "Z"].into_iter().map(String::from).collect();
    let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
    let result = restore_display_lines(&clean, &display, &refs);
    assert_eq!(result, "X\nY\nZ");
}

#[test]
fn restore_display_first_and_last_removed() {
    let clean: Vec<String> = vec!["a", "b", "c", "d"]
        .into_iter()
        .map(String::from)
        .collect();
    let display: Vec<String> = vec!["A", "B", "C", "D"]
        .into_iter()
        .map(String::from)
        .collect();
    let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
    let survivors = vec![refs[1], refs[2]];
    let result = restore_display_lines(&clean, &display, &survivors);
    assert_eq!(result, "B\nC");
}

#[test]
fn restore_display_single_line() {
    let clean: Vec<String> = vec![String::from("only")];
    let display: Vec<String> = vec![String::from("ONLY")];
    let refs: Vec<&str> = clean.iter().map(String::as_str).collect();
    let result = restore_display_lines(&clean, &display, &refs);
    assert_eq!(result, "ONLY");
}
