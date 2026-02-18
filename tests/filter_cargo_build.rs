#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/cargo/build.toml", env!("CARGO_MANIFEST_DIR"));
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
fn cargo_build_success_shows_ok() {
    let config = load_config();
    let fixture = load_fixture("cargo_build_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "ok ✓");
}

#[test]
fn cargo_build_failure_shows_error_lines() {
    let config = load_config();
    let fixture = load_fixture("cargo_build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result);
    // on_failure has tail = 30; fixture has error lines after compile noise is stripped
    assert!(!filtered.output.is_empty());
    assert!(
        filtered.output.contains("error"),
        "expected error in failure output, got: {}",
        filtered.output
    );
}

#[test]
fn cargo_build_skips_compiling_lines() {
    let config = load_config();
    let fixture = load_fixture("cargo_build_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    // on_success output = "ok ✓" — compile noise is gone
    assert!(!filtered.output.contains("Compiling"));
    assert!(!filtered.output.contains("Checking"));
}
