#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!(
        "{}/filters/gradle/dependencies.toml",
        env!("CARGO_MANIFEST_DIR")
    );
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
fn dependencies_removes_task_header() {
    let config = load_config();
    let fixture = load_fixture("gradle/dependencies.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("> Task"),
        "expected no task headers, got: {}",
        filtered.output
    );
}

#[test]
fn dependencies_removes_asterisk_lines() {
    let config = load_config();
    let fixture = load_fixture("gradle/dependencies.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("(*)"),
        "expected no (*) lines, got lines with (*)"
    );
}

#[test]
fn dependencies_preserves_tree_lines() {
    let config = load_config();
    let fixture = load_fixture("gradle/dependencies.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("+---") || filtered.output.contains("\\---"),
        "expected tree lines (+--- or \\---), got: {}",
        &filtered.output[..filtered.output.len().min(200)]
    );
}

#[test]
fn dependencies_preserves_config_headers() {
    let config = load_config();
    let fixture = load_fixture("gradle/dependencies.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("compileClasspath"),
        "expected 'compileClasspath' config header, got: {}",
        &filtered.output[..filtered.output.len().min(200)]
    );
}
