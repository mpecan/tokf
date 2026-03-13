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

// --- RTK serde alias tests ---

#[test]
fn apply_rtk_strip_lines_matching_alias() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_lines_matching = ["^noise"]
"#,
    )
    .unwrap();
    let result = make_result("noise line\nkeep me\nnoise again", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "keep me"
    );
}

#[test]
fn apply_rtk_keep_lines_matching_alias() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
keep_lines_matching = ["^keep"]
"#,
    )
    .unwrap();
    let result = make_result("drop me\nkeep this\ndrop too", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "keep this"
    );
}

// --- RTK match_output with regex pattern ---

#[test]
fn apply_rtk_match_output_pattern_regex() {
    // RTK patterns span lines using literal \n (not .* with dotall).
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[[match_output]]
pattern = "0 Warning\\(s\\)\\n\\s+0 Error\\(s\\)"
message = "ok (build succeeded)"
"#,
    )
    .unwrap();
    let result = make_result("  0 Warning(s)\n  0 Error(s)\nDone", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "ok (build succeeded)"
    );
}

#[test]
fn apply_rtk_match_output_unless_guard() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[[match_output]]
pattern = "total size is"
output = "ok (synced)"
unless = "error|failed"
"#,
    )
    .unwrap();

    // Without error → matches
    let result = make_result("total size is 42\nall good", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "ok (synced)"
    );

    // With error → unless fires, falls through
    let result = make_result("total size is 42\nerror: broken", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "total size is 42\nerror: broken"
    );
}

// --- RTK replace with $N syntax ---

#[test]
fn apply_rtk_replace_with_dollar_syntax() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[[replace]]
pattern = '^(\S+)\s+(\S+)\s+(\S+)'
replacement = "$1: $2 -> $3"
"#,
    )
    .unwrap();
    let result = make_result("pkg  1.0  2.0", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "pkg: 1.0 -> 2.0"
    );
}

// --- RTK head_lines / tail_lines aliases ---

#[test]
fn apply_rtk_head_lines_alias() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
head_lines = 2
"#,
    )
    .unwrap();
    let result = make_result("a\nb\nc\nd", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "a\nb"
    );
}

#[test]
fn apply_rtk_tail_lines_alias() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
tail_lines = 2
"#,
    )
    .unwrap();
    let result = make_result("a\nb\nc\nd", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "c\nd"
    );
}

// --- max_lines ---

#[test]
fn apply_max_lines_caps_output() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
max_lines = 3
"#,
    )
    .unwrap();
    let result = make_result("a\nb\nc\nd\ne", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "a\nb\nc"
    );
}

#[test]
fn apply_max_lines_after_head() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
head_lines = 4
max_lines = 2
"#,
    )
    .unwrap();
    let result = make_result("a\nb\nc\nd\ne", 0);
    // head=4 → [a,b,c,d], then max_lines=2 → [a,b]
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "a\nb"
    );
}

// --- Full RTK-style filter simulation ---

#[test]
fn apply_rtk_full_filter_example() {
    // Simulates a complete RTK-style filter: strip_lines_matching + match_output + on_empty
    let config: FilterConfig = toml::from_str(
        r#"
command = "make"
description = "Compact make output"
strip_lines_matching = [
  "^make\\[\\d+\\]:",
  "^\\s*$",
  "^Nothing to be done",
]
max_lines = 50
on_empty = "make: ok"
"#,
    )
    .unwrap();

    // All lines stripped → on_empty fires
    let result = make_result(
        "make[1]: Entering directory\n\nNothing to be done for 'all'",
        0,
    );
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "make: ok"
    );

    // Real content survives
    let result2 = make_result("make[1]: Entering\ncc -o main main.c\nmake[1]: Leaving", 0);
    assert_eq!(
        apply(&config, &result2, &[], &FilterOptions::default()).output,
        "cc -o main main.c"
    );
}
