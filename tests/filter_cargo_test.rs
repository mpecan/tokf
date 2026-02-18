#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/cargo-test.toml", env!("CARGO_MANIFEST_DIR"));
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
fn cargo_test_pass_aggregates_summaries() {
    let config = load_config();
    let fixture = load_fixture("cargo_test_pass.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    // 12 + 8 + 3 = 23 passed across 3 suites
    assert_eq!(filtered.output, "\u{2713} cargo test: 23 passed (3 suites)");
}

#[test]
fn cargo_test_fail_shows_failure_details() {
    let config = load_config();
    let fixture = load_fixture("cargo_test_fail.txt");
    let result = make_result(&fixture, 101);
    let filtered = filter::apply(&config, &result);

    assert!(
        filtered.output.starts_with("FAILURES (2):"),
        "expected output to start with 'FAILURES (2):', got:\n{}",
        filtered.output
    );
    assert!(
        filtered.output.contains("1."),
        "expected numbered failure 1"
    );
    assert!(
        filtered.output.contains("2."),
        "expected numbered failure 2"
    );
    assert!(
        filtered.output.contains("branch_fixed_output"),
        "expected failure name branch_fixed_output"
    );
    assert!(
        filtered.output.contains("extract_first_match"),
        "expected failure name extract_first_match"
    );
    assert!(
        filtered.output.contains("test result: FAILED."),
        "expected summary line"
    );
}

#[test]
fn cargo_test_compile_error_falls_back_to_tail() {
    let config = load_config();
    let fixture = load_fixture("cargo_test_compile_error.txt");
    let result = make_result(&fixture, 101);
    let filtered = filter::apply(&config, &result);

    // No sections matched → fallback tail = 5
    let lines: Vec<&str> = filtered.output.lines().collect();
    assert_eq!(lines.len(), 5, "expected 5 lines from fallback tail");
    assert!(
        filtered.output.contains("could not compile `tokf` (lib)"),
        "expected compiler error in tail"
    );
}
