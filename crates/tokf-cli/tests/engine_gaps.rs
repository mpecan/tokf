//! Integration tests for Issue #18 engine gap closures:
//! Gap 1 — per-line [[replace]] rules
//! Gap 3 — stateful dedup
//! Gap 5 — template sub-filtering pipes (lines, keep, where)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn config(toml: &str) -> FilterConfig {
    toml::from_str(toml).unwrap()
}

fn result(output: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined: output.to_string(),
    }
}

fn load_filter(path: &str) -> FilterConfig {
    let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&full).unwrap();
    toml::from_str(&content).unwrap()
}

fn load_fixture(path: &str) -> String {
    let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&full)
        .unwrap()
        .trim_end()
        .to_string()
}

// ---------------------------------------------------------------------------
// Gap 1 — [[replace]]
// ---------------------------------------------------------------------------

#[test]
fn replace_reformats_columns() {
    let cfg = config(
        r#"
command = "test"

[[replace]]
pattern = '^(\S+)\s+(\S+)\s+(\S+)'
output = "{1}: {2} → {3}"

[on_success]
output = "{output}"
"#,
    );
    let r = result("pkg  1.0  2.0\nother  a  b", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "pkg: 1.0 → 2.0\nother: a → b");
}

#[test]
fn replace_multiple_rules_chain() {
    let cfg = config(
        r#"
command = "test"

[[replace]]
pattern = 'foo'
output = "bar"

[[replace]]
pattern = 'bar'
output = "baz"

[on_success]
output = "{output}"
"#,
    );
    let r = result("foo\nnoop", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "baz\nnoop");
}

#[test]
fn replace_no_match_passthrough() {
    let cfg = config(
        r#"
command = "test"

[[replace]]
pattern = 'NOMATCH'
output = "replaced"

[on_success]
output = "{output}"
"#,
    );
    let r = result("hello world", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "hello world");
}

// ---------------------------------------------------------------------------
// Gap 3 — dedup
// ---------------------------------------------------------------------------

#[test]
fn dedup_collapses_consecutive() {
    let cfg = config(
        r#"
command = "test"
dedup = true

[on_success]
output = "{output}"
"#,
    );
    let r = result("a\na\nb\nb", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "a\nb");
}

#[test]
fn dedup_window_drops_within_window() {
    let cfg = config(
        r#"
command = "test"
dedup = true
dedup_window = 3

[on_success]
output = "{output}"
"#,
    );
    // "a" reappears within window of 3 → dropped
    let r = result("a\nb\na", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "a\nb");
}

// ---------------------------------------------------------------------------
// Gap 5 — lines, keep, where pipes
// ---------------------------------------------------------------------------

#[test]
fn pipe_lines_and_keep_filter() {
    let cfg = config(
        r#"
command = "test"

[on_success]
output = "{output | lines | keep: \"^error\" | join: \"\\n\"}"
"#,
    );
    let r = result("good line\nerror: bad\ngood again", 0);
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "error: bad");
}

#[test]
fn pipe_where_alias_filters_lines() {
    let cfg = config(
        r#"
command = "test"

[on_success]
output = "{output | lines | where: \"^WARN\" | join: \"\\n\"}"
"#,
    );
    let r = result(
        "INFO startup\nWARN: disk low\nINFO done\nWARN: high memory",
        0,
    );
    let out = filter::apply(&cfg, &r, &[]).output;
    assert_eq!(out, "WARN: disk low\nWARN: high memory");
}

// ---------------------------------------------------------------------------
// Gap 5 — pytest filter with lines + keep
// ---------------------------------------------------------------------------

#[test]
fn pytest_gap5_filter_shows_only_assertion_lines() {
    let cfg = load_filter("filters/pytest.toml");
    let fixture = load_fixture("tests/fixtures/pytest/fail.txt");
    let r = result(&fixture, 1);
    let out = filter::apply(&cfg, &r, &[]).output;

    // The > (pointer) line must appear
    assert!(
        out.contains(">       assert x == 3"),
        "expected pointer line in output:\n{out}"
    );
    // The E (error) line must appear
    assert!(
        out.contains("E       AssertionError: assert 2 == 3"),
        "expected assertion error line in output:\n{out}"
    );
    // The FAILED summary line must appear
    assert!(
        out.contains("FAILED tests/test_example.py::test_addition"),
        "expected FAILED summary line in output:\n{out}"
    );
    // Raw Python source lines must NOT appear literally
    assert!(
        !out.contains("def test_addition():"),
        "unexpected source code in output:\n{out}"
    );
}
