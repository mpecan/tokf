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

// --- apply (full pipeline) ---

#[test]
fn apply_match_output_short_circuits() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
match_output = [
  { contains = "special", output = "found it" },
]

[on_success]
output = "should not reach"
"#,
    )
    .unwrap();

    let result = make_result("some special output", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "found it"
    );
}

#[test]
fn apply_passthrough_no_branch() {
    let config: FilterConfig = toml::from_str(r#"command = "test""#).unwrap();
    let result = make_result("raw output", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "raw output"
    );
}

#[test]
fn apply_success_branch() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
[on_success]
output = "ok"
"#,
    )
    .unwrap();

    let result = make_result("anything", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "ok"
    );
}

#[test]
fn apply_failure_branch() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
[on_failure]
tail = 2
"#,
    )
    .unwrap();

    let result = make_result("a\nb\nc\nd", 1);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "c\nd"
    );
}

#[test]
fn apply_full_skip_then_extract() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[on_success]
skip = ["^noise"]
extract = { pattern = '(\w+) -> (\w+)', output = "pushed {2}" }
"#,
    )
    .unwrap();

    let result = make_result("noise line\nmain -> main\nnoise again", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "pushed main"
    );
}

// --- parse pipeline tests ---

#[test]
fn apply_parse_overrides_on_success() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }

[on_success]
output = "should not appear"
"#,
    )
    .unwrap();

    let result = make_result("## main", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "main\n"
    );
}

#[test]
fn apply_parse_overrides_on_failure() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }

[on_failure]
output = "should not appear"
"#,
    )
    .unwrap();

    let result = make_result("## develop", 1);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "develop\n"
    );
}

#[test]
fn apply_match_output_overrides_parse() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
match_output = [
  { contains = "fatal", output = "error!" },
]

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }
"#,
    )
    .unwrap();

    let result = make_result("fatal: something broke", 128);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "error!"
    );
}

#[test]
fn apply_top_level_skip_affects_parse() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^#"]

[parse]
branch = { line = 1, pattern = '^(\S+)', output = "{1}" }
"#,
    )
    .unwrap();

    // After skip removes "# comment", the first line becomes "M  file.rs"
    let result = make_result("# comment\nM  file.rs", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "M\n"
    );
}

#[test]
fn apply_top_level_keep_affects_branch_path() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
keep = ["^keep"]
"#,
    )
    .unwrap();

    let result = make_result("drop me\nkeep this\ndrop too\nkeep that", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "keep this\nkeep that"
    );
}

#[test]
fn apply_output_var_passthrough() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
[on_success]
output = "{output}"
"#,
    )
    .unwrap();

    let result = make_result("line1\nline2\nline3", 0);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "line1\nline2\nline3"
    );
}

#[test]
fn apply_output_var_with_skip_prefiltering() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
skip = ["^#"]
[on_success]
output = "{output}"
"#,
    )
    .unwrap();

    let result = make_result("# comment\nreal line\n# another", 0);
    // {output} resolves to pre-filtered output (skip applied)
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "real line"
    );
}

#[test]
fn apply_output_var_in_failure_branch() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
[on_failure]
output = "FAILED:\n{output}"
"#,
    )
    .unwrap();

    let result = make_result("error: something broke\ndetails here", 1);
    assert_eq!(
        apply(&config, &result, &[], &FilterOptions::default()).output,
        "FAILED:\nerror: something broke\ndetails here"
    );
}

#[test]
fn apply_output_var_with_sections() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"

[[section]]
name = "items"
match = "^item:"
collect_as = "items"

[on_success]
output = "Found {items.count} items in:\n{output}"
"#,
    )
    .unwrap();

    let input = "header\nitem: one\nitem: two\nfooter";
    let result = make_result(input, 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(
        filtered.output,
        "Found 2 items in:\nheader\nitem: one\nitem: two\nfooter"
    );
}

// --- cleanup flag integration tests ---

#[test]
fn apply_strip_ansi_before_skip() {
    // ANSI codes stripped at stage 1.6, before skip patterns run at stage 2.
    // A skip pattern matching the plain text must fire even though the raw
    // line contained color codes.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_ansi = true
skip = ["^warning"]
"#,
    )
    .unwrap();
    let result = make_result("\x1b[33mwarning\x1b[0m: overflow\ninfo: ok", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "info: ok");
}

#[test]
fn apply_trim_lines_before_keep() {
    // trim_lines fires at stage 1.6, before keep at stage 2.
    // A keep pattern matching the trimmed text must retain the line.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
trim_lines = true
keep = ["^OK"]
"#,
    )
    .unwrap();
    let result = make_result("   OK done   \n   FAIL   ", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "OK done");
}

#[test]
fn apply_strip_empty_lines_after_branch_template() {
    // strip_empty_lines post-processes output from on_success branch template.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_empty_lines = true

[on_success]
output = "{output}"
"#,
    )
    .unwrap();
    let result = make_result("line1\n\nline2\n   \nline3", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "line1\nline2\nline3");
}

#[test]
fn apply_strip_empty_lines_on_match_output_path() {
    // match_output early-return also applies post_process_output.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_empty_lines = true

[[match_output]]
contains = "sentinel"
output = "header\n\nbody\n\nfooter"
"#,
    )
    .unwrap();
    let result = make_result("sentinel found", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "header\nbody\nfooter");
}

#[test]
fn apply_collapse_empty_lines_after_branch() {
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
collapse_empty_lines = true

[on_success]
output = "{output}"
"#,
    )
    .unwrap();
    let result = make_result("a\n\n\n\nb", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "a\n\nb");
}

#[test]
fn apply_strip_ansi_then_dedup() {
    // Cleanup (1.6) runs before dedup (2.5).
    // Two ANSI-colored identical lines should be deduplicated after stripping.
    let config: FilterConfig = toml::from_str(
        r#"
command = "test"
strip_ansi = true
dedup = true
"#,
    )
    .unwrap();
    let result = make_result("\x1b[33ma\x1b[0m\n\x1b[33ma\x1b[0m\nb", 0);
    let filtered = apply(&config, &result, &[], &FilterOptions::default());
    assert_eq!(filtered.output, "a\nb");
}
