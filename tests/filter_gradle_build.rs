#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/gradle/build.toml", env!("CARGO_MANIFEST_DIR"));
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
fn build_incremental_extracts_time() {
    let config = load_config();
    let fixture = load_fixture("gradle/build_incremental.txt");
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
fn build_clean_extracts_time() {
    let config = load_config();
    let fixture = load_fixture("gradle/build_clean.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.starts_with("ok ✓ "),
        "expected 'ok ✓ <time>', got: {}",
        filtered.output
    );
}

#[test]
fn build_clean_removes_passed_lines() {
    let config = load_config();
    let fixture = load_fixture("gradle/build_clean.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains(" PASSED"),
        "expected no PASSED lines, got: {}",
        filtered.output
    );
}

#[test]
fn build_failure_shows_failed_task() {
    let config = load_config();
    let fixture = load_fixture("gradle/build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("FAILED"),
        "expected FAILED in output, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("BUILD FAILED"),
        "expected BUILD FAILED in output, got: {}",
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
fn build_failure_respects_tail() {
    let config = load_config();
    let fixture = load_fixture("gradle/build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    let line_count = filtered.output.lines().count();
    assert!(
        line_count <= 20,
        "expected at most 20 lines, got: {line_count}"
    );
    // Verify tail actually truncated: early warnings (before the cutoff) must be absent.
    // The fixture has >20 surviving lines; the first warnings referencing UpsertEntity.kt
    // and DatabaseConfig.kt fall before the tail window.
    assert!(
        !filtered.output.contains("UpsertEntity.kt"),
        "expected early warning lines to be truncated by tail=20"
    );
    assert!(
        !filtered.output.contains("DatabaseConfig.kt"),
        "expected early warning lines to be truncated by tail=20"
    );
}
