#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/gradle/test.toml", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap();
    toml::from_str(&content).unwrap()
}

fn load_fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap()
        .trim_end()
        .to_string()
}

fn make_result(fixture: &str, exit_code: i32) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined: fixture.to_string(),
    }
}

#[test]
fn test_incremental_extracts_time() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_incremental.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.starts_with("ok ✓ "),
        "expected 'ok ✓ <time>', got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("> Task"),
        "expected no task headers in output, got: {}",
        filtered.output
    );
}

#[test]
fn test_clean_extracts_time() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_clean.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.starts_with("ok ✓ "),
        "expected 'ok ✓ <time>', got: {}",
        filtered.output
    );
}

#[test]
fn test_clean_skips_passed_lines() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_clean.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains(" PASSED"),
        "expected no PASSED lines, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.is_empty(),
        "expected non-empty output after filtering PASSED lines"
    );
}

#[test]
fn test_failure_shows_assertion() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("AssertionFailedError"),
        "expected AssertionFailedError in output, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("UP-TO-DATE") && !filtered.output.contains("NO-SOURCE"),
        "expected task status lines to be stripped, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("SKIPPED"),
        "expected SKIPPED task lines to be stripped, got: {}",
        filtered.output
    );
}

#[test]
fn test_failure_shows_summary() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("tests completed"),
        "expected 'tests completed' in output, got: {}",
        filtered.output
    );
}

#[test]
fn test_failure_respects_tail() {
    let config = load_config();
    let fixture = load_fixture("gradle/test_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    let line_count = filtered.output.lines().count();
    assert!(
        line_count <= 30,
        "expected at most 30 lines, got: {line_count}"
    );
}
