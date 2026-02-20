#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config() -> FilterConfig {
    let path = format!("{}/filters/docker/compose.toml", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap();
    toml::from_str(&content).unwrap()
}

fn load_fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/docker/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
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
fn docker_compose_startup_shows_errors_and_ready() {
    let config = load_config();
    let fixture = load_fixture("compose-startup.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);

    assert!(filtered.output.contains("ERROR"));
    assert!(
        filtered
            .output
            .contains("database system is ready to accept connections")
    );
    assert!(filtered.output.contains("Ready on http://localhost:3000"));
    assert!(!filtered.output.contains("⠿ Network"));
    assert!(!filtered.output.contains("⠿ Container"));
}
