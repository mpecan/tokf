use super::*;
use crate::config::types::ExtractRule;

fn make_result(combined: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined: combined.to_string(),
    }
}

fn minimal_config() -> FilterConfig {
    toml::from_str(r#"command = "test""#).unwrap()
}

// --- select_branch ---

#[test]
fn select_branch_success() {
    let mut config = minimal_config();
    config.on_success = Some(OutputBranch {
        output: Some("success".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    });
    assert!(select_branch(&config, 0).is_some());
    assert!(select_branch(&config, 1).is_none());
}

#[test]
fn select_branch_failure() {
    let mut config = minimal_config();
    config.on_failure = Some(OutputBranch {
        output: Some("failure".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    });
    assert!(select_branch(&config, 0).is_none());
    assert!(select_branch(&config, 1).is_some());
    assert!(select_branch(&config, 127).is_some());
}

// --- apply_branch ---

/// Helper: call apply_branch with empty sections (non-section path).
fn branch_apply(branch: &OutputBranch, combined: &str) -> String {
    apply_branch(branch, combined, &SectionMap::new(), false).unwrap()
}

#[test]
fn branch_fixed_output() {
    let branch = OutputBranch {
        output: Some("ok \u{2713}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "anything"), "ok \u{2713}");
}

#[test]
fn branch_output_template_resolves_output_var() {
    let branch = OutputBranch {
        output: Some("{output}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "hello world"), "hello world");
}

#[test]
fn branch_output_template_with_surrounding_text() {
    let branch = OutputBranch {
        output: Some("Result: {output}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(
        branch_apply(&branch, "line1\nline2"),
        "Result: line1\nline2"
    );
}

#[test]
fn branch_tail_truncation() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(2),
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "c\nd");
}

#[test]
fn branch_head_truncation() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: Some(2),
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "a\nb");
}

#[test]
fn branch_tail_then_head() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(3),
        head: Some(2),
        skip: vec![],
        extract: None,
    };
    // tail 3 of [a,b,c,d] → [b,c,d], then head 2 → [b,c]
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "b\nc");
}

#[test]
fn branch_skip_then_join() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec!["^noise".to_string()],
        extract: None,
    };
    assert_eq!(
        branch_apply(&branch, "noise line\nkeep me\nnoise again"),
        "keep me"
    );
}

#[test]
fn branch_extract() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: Some(ExtractRule {
            pattern: r"(\S+)\s*->\s*(\S+)".to_string(),
            output: "ok {2}".to_string(),
        }),
    };
    assert_eq!(branch_apply(&branch, "main -> main"), "ok main");
}

#[test]
fn branch_tail_less_than_lines() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(10),
        head: None,
        skip: vec![],
        extract: None,
    };
    // Only 3 lines, tail 10 → all lines kept
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "a\nb\nc");
}

#[test]
fn branch_empty_string_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, ""), "");
}

#[test]
fn branch_single_line_no_newline() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "only-line"), "only-line");
}

#[test]
fn branch_tail_zero_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(0),
        head: None,
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "");
}

#[test]
fn branch_head_zero_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: Some(0),
        skip: vec![],
        extract: None,
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "");
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
    assert_eq!(apply(&config, &result, &[]).output, "found it");
}

#[test]
fn apply_passthrough_no_branch() {
    let config = minimal_config();
    let result = make_result("raw output", 0);
    assert_eq!(apply(&config, &result, &[]).output, "raw output");
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
    assert_eq!(apply(&config, &result, &[]).output, "ok");
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
    assert_eq!(apply(&config, &result, &[]).output, "c\nd");
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
    assert_eq!(apply(&config, &result, &[]).output, "pushed main");
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
    assert_eq!(apply(&config, &result, &[]).output, "main\n");
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
    assert_eq!(apply(&config, &result, &[]).output, "develop\n");
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
    assert_eq!(apply(&config, &result, &[]).output, "error!");
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
    assert_eq!(apply(&config, &result, &[]).output, "M\n");
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
    assert_eq!(apply(&config, &result, &[]).output, "keep this\nkeep that");
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
    assert_eq!(apply(&config, &result, &[]).output, "line1\nline2\nline3");
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
    assert_eq!(apply(&config, &result, &[]).output, "real line");
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
        apply(&config, &result, &[]).output,
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
    let filtered = apply(&config, &result, &[]);
    assert_eq!(
        filtered.output,
        "Found 2 items in:\nheader\nitem: one\nitem: two\nfooter"
    );
}
