#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/git/diff.toml", env!("CARGO_MANIFEST_DIR"));
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
fn git_diff_stat_passthrough() {
    let config = load_config();
    let fixture = load_fixture("git_diff_stat.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, fixture);
}

#[test]
fn git_diff_empty_no_changes() {
    let config = load_config();
    let fixture = load_fixture("git_diff_empty.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "");
}

#[test]
fn git_diff_fatal_match_output() {
    let config = load_config();
    let fixture = load_fixture("git_diff_failure.txt");
    // fatal: triggers match_output regardless of exit code
    let result = make_result(&fixture, 128);
    let filtered = filter::apply(&config, &result);
    // {line_containing} resolves to the line containing "fatal:"
    assert_eq!(
        filtered.output,
        "\u{2717} fatal: bad revision 'nonexistent'"
    );
}

#[test]
fn git_diff_non_fatal_failure_tail() {
    let config = load_config();
    // A non-fatal failure (no "fatal:" substring) → on_failure branch
    let result = make_result("error: something\nwent wrong\ndetails here", 1);
    let filtered = filter::apply(&config, &result);
    // tail = 5, only 3 lines → all shown
    assert_eq!(
        filtered.output,
        "error: something\nwent wrong\ndetails here"
    );
}
