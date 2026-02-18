#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/git/push.toml", env!("CARGO_MANIFEST_DIR"));
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
fn git_push_success_extracts_branch() {
    let config = load_config();
    let fixture = load_fixture("git_push_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "ok \u{2713} main");
}

#[test]
fn git_push_up_to_date() {
    let config = load_config();
    let fixture = load_fixture("git_push_up_to_date.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "ok (up-to-date)");
}

#[test]
fn git_push_rejected() {
    let config = load_config();
    let fixture = load_fixture("git_push_rejected.txt");
    // rejected push may exit 0 or non-zero depending on git version,
    // but the match_output rule triggers regardless of exit code
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result);
    assert_eq!(
        filtered.output,
        "\u{2717} push rejected (try pulling first)"
    );
}

#[test]
fn git_push_failure_passthrough_tail() {
    let config = load_config();
    let fixture = load_fixture("git_push_failure.txt");
    let result = make_result(&fixture, 128);
    let filtered = filter::apply(&config, &result);
    // on_failure has tail = 10, but fixture has only 5 lines → all shown
    assert_eq!(filtered.output, fixture);
}
