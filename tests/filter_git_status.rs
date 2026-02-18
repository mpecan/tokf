#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/git/status.toml", env!("CARGO_MANIFEST_DIR"));
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
fn git_status_normal() {
    let config = load_config();
    let fixture = load_fixture("git_status_normal.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(
        filtered.output,
        "main\n  modified: 1\n  modified (unstaged): 1\n  untracked: 2"
    );
}

#[test]
fn git_status_clean() {
    let config = load_config();
    let fixture = load_fixture("git_status_clean.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "main\nclean \u{2014} nothing to commit");
}

#[test]
fn git_status_not_a_repo() {
    let config = load_config();
    let fixture = load_fixture("git_status_not_repo.txt");
    let result = make_result(&fixture, 128);
    let filtered = filter::apply(&config, &result);
    assert_eq!(filtered.output, "Not a git repository");
}

#[test]
fn git_status_all_types() {
    let config = load_config();
    let fixture = load_fixture("git_status_all_types.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result);

    // Exact expected output — alphabetically sorted labels with correct counts
    let expected = "\
feature-branch
  added: 1
  added+modified: 1
  conflict: 1
  deleted: 1
  deleted (unstaged): 1
  modified: 2
  modified (staged+unstaged): 1
  modified (unstaged): 1
  renamed: 1
  untracked: 2";

    assert_eq!(filtered.output, expected);
}
