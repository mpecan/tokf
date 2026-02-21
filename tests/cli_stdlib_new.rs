#![allow(clippy::unwrap_used, clippy::expect_used)]

use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::runner::CommandResult;

fn load_config(path: &str) -> FilterConfig {
    let full = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path);
    let content = std::fs::read_to_string(&full).unwrap();
    toml::from_str(&content).unwrap()
}

fn load_fixture(rel: &str) -> String {
    let full = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), rel);
    std::fs::read_to_string(&full)
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

// --- git/show ---

#[test]
fn git_show_success_keeps_stat_output() {
    let config = load_config("filters/git/show.toml");
    let fixture = load_fixture("git/show_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("files changed"),
        "expected stat summary, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("feat(auth)"),
        "expected commit message, got: {}",
        filtered.output
    );
}

#[test]
fn git_show_failure_shows_tail() {
    let config = load_config("filters/git/show.toml");
    let fixture = load_fixture("git/show_failure.txt");
    let result = make_result(&fixture, 128);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("fatal"),
        "expected error message, got: {}",
        filtered.output
    );
}

// --- cargo/check ---

#[test]
fn cargo_check_success_shows_ok() {
    let config = load_config("filters/cargo/check.toml");
    let fixture = load_fixture("cargo/check_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert_eq!(filtered.output, "✓ cargo check: ok");
}

#[test]
fn cargo_check_failure_shows_error() {
    let config = load_config("filters/cargo/check.toml");
    let fixture = load_fixture("cargo/check_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("error"),
        "expected error in output, got: {}",
        filtered.output
    );
}

#[test]
fn cargo_check_strips_compiling_lines() {
    let config = load_config("filters/cargo/check.toml");
    let fixture = load_fixture("cargo/check_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(!filtered.output.contains("Compiling"));
    assert!(!filtered.output.contains("Checking"));
}

// --- cargo/install ---

#[test]
fn cargo_install_success_strips_noise() {
    let config = load_config("filters/cargo/install.toml");
    let fixture = load_fixture("cargo/install_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("Compiling"),
        "expected Compiling to be stripped, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("Downloading"),
        "expected Downloading to be stripped, got: {}",
        filtered.output
    );
}

#[test]
fn cargo_install_success_keeps_installed_line() {
    let config = load_config("filters/cargo/install.toml");
    let fixture = load_fixture("cargo/install_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Installed") || filtered.output.contains("Installing"),
        "expected installed/installing line, got: {}",
        filtered.output
    );
}

#[test]
fn cargo_install_failure_shows_error() {
    let config = load_config("filters/cargo/install.toml");
    let fixture = load_fixture("cargo/install_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("error"),
        "expected error in output, got: {}",
        filtered.output
    );
}

// --- npm/run ---

#[test]
fn npm_run_skips_project_header_line() {
    let config = load_config("filters/npm/run.toml");
    let fixture = load_fixture("npm/run_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("myproject@1.0.0"),
        "expected project header stripped, got: {}",
        filtered.output
    );
}

#[test]
fn npm_run_success_keeps_build_output() {
    let config = load_config("filters/npm/run.toml");
    let fixture = load_fixture("npm/run_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Done") || filtered.output.contains("Compiled"),
        "expected build output, got: {}",
        filtered.output
    );
}

#[test]
fn npm_run_failure_strips_header_shows_errors() {
    let config = load_config("filters/npm/run.toml");
    let fixture = load_fixture("npm/run_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("myproject@1.0.0"),
        "expected header stripped, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("Failed") || filtered.output.contains("error"),
        "expected error content, got: {}",
        filtered.output
    );
}

#[test]
fn npm_run_strips_npm_warn_lines() {
    let config = load_config("filters/npm/run.toml");
    let fixture = load_fixture("npm/run_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.to_lowercase().contains("npm warn"),
        "expected npm warn lines stripped, got: {}",
        filtered.output
    );
}

// --- pnpm/install ---

#[test]
fn pnpm_install_success_strips_progress() {
    let config = load_config("filters/pnpm/install.toml");
    let fixture = load_fixture("pnpm/install_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("Progress:"),
        "expected Progress: stripped, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("Already up to date"),
        "expected Already up to date stripped, got: {}",
        filtered.output
    );
}

#[test]
fn pnpm_install_failure_shows_error() {
    let config = load_config("filters/pnpm/install.toml");
    let fixture = load_fixture("pnpm/install_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("ERR") || filtered.output.contains("peer"),
        "expected error content, got: {}",
        filtered.output
    );
}

// --- pnpm/add ---

#[test]
fn pnpm_add_config_parses_correctly() {
    let config = load_config("filters/pnpm/add.toml");
    assert_eq!(config.command.first(), "pnpm add *");
}

#[test]
fn pnpm_add_success_strips_noise() {
    let config = load_config("filters/pnpm/add.toml");
    let fixture = load_fixture("pnpm/add_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("Progress:"),
        "expected Progress: stripped, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("lodash"),
        "expected added package in output, got: {}",
        filtered.output
    );
}

#[test]
fn pnpm_add_failure_shows_error() {
    let config = load_config("filters/pnpm/add.toml");
    let fixture = load_fixture("pnpm/add_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("ERR_PNPM_NO_MATCHING_VERSION"),
        "expected error code in output, got: {}",
        filtered.output
    );
}

// --- go/build ---

#[test]
fn go_build_success_shows_ok() {
    let config = load_config("filters/go/build.toml");
    let fixture = load_fixture("go/build_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert_eq!(filtered.output, "✓ go build: ok");
}

#[test]
fn go_build_failure_keeps_error_lines() {
    let config = load_config("filters/go/build.toml");
    let fixture = load_fixture("go/build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("undefined"),
        "expected go error, got: {}",
        filtered.output
    );
}

#[test]
fn go_build_failure_strips_package_header() {
    let config = load_config("filters/go/build.toml");
    let fixture = load_fixture("go/build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("# mypackage"),
        "expected package header stripped, got: {}",
        filtered.output
    );
}

// --- go/vet ---

#[test]
fn go_vet_success_shows_ok() {
    let config = load_config("filters/go/vet.toml");
    let fixture = load_fixture("go/vet_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert_eq!(filtered.output, "✓ go vet: ok");
}

#[test]
fn go_vet_failure_keeps_vet_lines() {
    let config = load_config("filters/go/vet.toml");
    let fixture = load_fixture("go/vet_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Printf") || filtered.output.contains("sync"),
        "expected vet diagnostic, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("# mypackage"),
        "expected package header stripped, got: {}",
        filtered.output
    );
}

// --- pytest ---

#[test]
fn pytest_pass_extracts_count() {
    let config = load_config("filters/pytest.toml");
    let fixture = load_fixture("pytest/pass.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert_eq!(filtered.output, "✓ pytest: 20 passed");
}

#[test]
fn pytest_fail_shows_failure_context() {
    let config = load_config("filters/pytest.toml");
    let fixture = load_fixture("pytest/fail.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("FAILED"),
        "expected FAILED line in output, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("AssertionError"),
        "expected AssertionError in output, got: {}",
        filtered.output
    );
}

// --- tsc ---

#[test]
fn tsc_pass_short_circuits_found_zero_errors() {
    let config = load_config("filters/tsc.toml");
    let fixture = load_fixture("tsc/pass.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert_eq!(filtered.output, "✓ TypeScript: no errors");
}

#[test]
fn tsc_errors_keeps_error_lines() {
    let config = load_config("filters/tsc.toml");
    let fixture = load_fixture("tsc/errors.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("TS2322"),
        "expected TS2322 error, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("TS2305"),
        "expected TS2305 error, got: {}",
        filtered.output
    );
}

#[test]
fn tsc_errors_strips_found_n_errors_line() {
    let config = load_config("filters/tsc.toml");
    let fixture = load_fixture("tsc/errors.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("Found 3 errors"),
        "expected 'Found 3 errors' to be stripped, got: {}",
        filtered.output
    );
}

// --- docker/ps ---

#[test]
fn docker_ps_success_shows_containers() {
    let config = load_config("filters/docker/ps.toml");
    let fixture = load_fixture("docker/ps.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("NAMES"),
        "expected compact header row, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("nginx"),
        "expected nginx container, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("CONTAINER ID"),
        "expected no CONTAINER ID column, got: {}",
        filtered.output
    );
}

#[test]
fn docker_ps_failure_shows_error() {
    let config = load_config("filters/docker/ps.toml");
    let fixture = load_fixture("docker/ps_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Docker daemon not running"),
        "expected condensed daemon error, got: {}",
        filtered.output
    );
}

// --- docker/images ---

#[test]
fn docker_images_success_shows_images() {
    let config = load_config("filters/docker/images.toml");
    let fixture = load_fixture("docker/images.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("nginx: latest (187MB)"),
        "expected grouped nginx, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("postgres: 15 (379MB)"),
        "expected grouped postgres, got: {}",
        filtered.output
    );
}

#[test]
fn docker_images_failure_shows_error() {
    let config = load_config("filters/docker/images.toml");
    let fixture = load_fixture("docker/images_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered
            .output
            .contains("Cannot connect to the Docker daemon"),
        "expected daemon error, got: {}",
        filtered.output
    );
}

// --- kubectl/get ---

#[test]
fn kubectl_get_success_shows_resources() {
    let config = load_config("filters/kubectl/get.toml");
    let fixture = load_fixture("kubectl/get.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("NAME"),
        "expected header row, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("Running"),
        "expected Running status, got: {}",
        filtered.output
    );
}

#[test]
fn kubectl_get_failure_shows_error() {
    let config = load_config("filters/kubectl/get.toml");
    let fixture = load_fixture("kubectl/get_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Error from server"),
        "expected server error, got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("NotFound"),
        "expected NotFound status, got: {}",
        filtered.output
    );
}

// --- gh/pr ---

#[test]
fn gh_pr_list_strips_timestamps() {
    let config = load_config("filters/gh/pr/list.toml");
    let fixture = load_fixture("gh/pr.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("#12"),
        "expected PR #12, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("about 2 days ago"),
        "expected timestamps stripped, got: {}",
        filtered.output
    );
}

#[test]
fn gh_pr_list_failure_shows_error() {
    let config = load_config("filters/gh/pr/list.toml");
    let fixture = load_fixture("gh/pr_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("GraphQL"),
        "expected GraphQL error, got: {}",
        filtered.output
    );
}

// --- gh/issue ---

#[test]
fn gh_issue_list_strips_timestamps() {
    let config = load_config("filters/gh/issue/list.toml");
    let fixture = load_fixture("gh/issue.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("#14"),
        "expected issue #14, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("about 1 day ago"),
        "expected timestamps stripped, got: {}",
        filtered.output
    );
}

#[test]
fn gh_issue_list_failure_shows_error() {
    let config = load_config("filters/gh/issue/list.toml");
    let fixture = load_fixture("gh/issue_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("GraphQL"),
        "expected GraphQL error, got: {}",
        filtered.output
    );
}

// --- next/build ---

#[test]
fn next_build_success_strips_info_lines() {
    let config = load_config("filters/next/build.toml");
    let fixture = load_fixture("next/build_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("info  -"),
        "expected 'info' lines stripped, got: {}",
        filtered.output
    );
}

#[test]
fn next_build_success_keeps_route_table() {
    let config = load_config("filters/next/build.toml");
    let fixture = load_fixture("next/build_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Route") || filtered.output.contains("Done"),
        "expected route table or done, got: {}",
        filtered.output
    );
}

#[test]
fn next_build_failure_shows_errors() {
    let config = load_config("filters/next/build.toml");
    let fixture = load_fixture("next/build_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("Failed") || filtered.output.contains("error"),
        "expected failure output, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("info  -"),
        "expected info lines stripped from failure output, got: {}",
        filtered.output
    );
}

// --- prisma/generate ---

#[test]
fn prisma_generate_success_shows_generated() {
    let config = load_config("filters/prisma/generate.toml");
    let fixture = load_fixture("prisma/generate_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.starts_with("✓ Generated"),
        "expected output to start with '✓ Generated', got: {}",
        filtered.output
    );
    assert!(
        filtered.output.contains("Prisma Client"),
        "expected 'Prisma Client' in output, got: {}",
        filtered.output
    );
}

#[test]
fn prisma_generate_success_strips_box_art() {
    let config = load_config("filters/prisma/generate.toml");
    let fixture = load_fixture("prisma/generate_success.txt");
    let result = make_result(&fixture, 0);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        !filtered.output.contains("┌") && !filtered.output.contains("│"),
        "expected box art stripped, got: {}",
        filtered.output
    );
    assert!(
        !filtered.output.contains("Prisma schema loaded"),
        "expected schema loaded line stripped, got: {}",
        filtered.output
    );
}

#[test]
fn prisma_generate_failure_shows_error() {
    let config = load_config("filters/prisma/generate.toml");
    let fixture = load_fixture("prisma/generate_failure.txt");
    let result = make_result(&fixture, 1);
    let filtered = filter::apply(&config, &result, &[]);
    assert!(
        filtered.output.contains("error") || filtered.output.contains("Validation"),
        "expected error content, got: {}",
        filtered.output
    );
}
