#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/git/log.toml", env!("CARGO_MANIFEST_DIR"));
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
fn git_log_success_passthrough() {
    let config = load_config();
    let fixture = load_fixture("git_log_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    // {output} should resolve to the full command output
    assert_eq!(filtered.output, fixture);
}

#[test]
fn git_log_empty_output() {
    let config = load_config();
    let result = make_result("", 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "");
}
