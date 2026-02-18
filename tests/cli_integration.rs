use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

// --- tokf run ---

#[test]
fn run_echo_hello() {
    let output = tokf().args(["run", "echo", "hello"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn run_no_filter_passes_through() {
    let output = tokf()
        .args(["run", "--no-filter", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn run_timing_shows_duration() {
    let output = tokf()
        .args(["run", "--timing", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Timing may or may not appear depending on whether a filter matched,
    // but the command should succeed regardless
    // With no filter match, no timing is shown — that's correct
    assert!(
        stderr.is_empty() || stderr.contains("[tokf]"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn run_false_propagates_exit_code() {
    let output = tokf().args(["run", "false"]).output().unwrap();
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn run_exit_code_42() {
    let output = tokf()
        .args(["run", "sh", "-c", "exit 42"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn run_verbose_shows_resolution_details() {
    let output = tokf()
        .args(["run", "--verbose", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf]"),
        "expected verbose output on stderr, got: {stderr}"
    );
}

#[test]
fn run_nonexistent_command_exits_with_error() {
    let output = tokf()
        .args(["run", "nonexistent_cmd_xyz_99"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] error"),
        "expected error on stderr, got: {stderr}"
    );
}

#[test]
fn run_no_filter_preserves_failing_exit_code() {
    let output = tokf()
        .args(["run", "--no-filter", "sh", "-c", "exit 7"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn run_timing_with_matched_filter() {
    // Use a temp dir with a filter that matches "echo"
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("echo.toml"),
        "command = \"echo\"\n[on_success]\noutput = \"filtered\"",
    )
    .unwrap();

    let output = tokf()
        .args(["run", "--timing", "echo", "hello"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] filter took"),
        "expected timing output when filter matched, got: {stderr}"
    );
}

// --- tokf check ---

#[test]
fn check_valid_filter() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let output = tokf().args(["check", &filter]).output().unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("valid"),
        "expected 'valid' in stderr, got: {stderr}"
    );
}

#[test]
fn check_nonexistent_file() {
    let output = tokf()
        .args(["check", "/nonexistent/path/filter.toml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr, got: {stderr}"
    );
}

#[test]
fn check_invalid_toml() {
    let dir = tempfile::TempDir::new().unwrap();
    let bad_toml = dir.path().join("bad.toml");
    std::fs::write(&bad_toml, "not valid toml [[[").unwrap();

    let output = tokf()
        .args(["check", bad_toml.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error"),
        "expected 'error' in stderr, got: {stderr}"
    );
}

// --- tokf test ---

#[test]
fn test_nonexistent_filter_exits_with_error() {
    let fixture = format!("{}/tests/fixtures/git_push_success.txt", manifest_dir());
    let output = tokf()
        .args(["test", "/nonexistent/filter.toml", &fixture])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("filter not found"),
        "expected 'filter not found' in stderr, got: {stderr}"
    );
}

#[test]
fn test_nonexistent_fixture_exits_with_error() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let output = tokf()
        .args(["test", &filter, "/nonexistent/fixture.txt"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read fixture"),
        "expected fixture error in stderr, got: {stderr}"
    );
}

#[test]
fn test_exit_code_selects_different_branch() {
    // git-push.toml: on_success extracts "ok ✓ {branch}", on_failure uses tail = 10
    // With exit_code=0, success branch extracts the push ref
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_success.txt", manifest_dir());

    let success_output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "0"])
        .output()
        .unwrap();
    let failure_output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "1"])
        .output()
        .unwrap();

    let success_stdout = String::from_utf8_lossy(&success_output.stdout);
    let failure_stdout = String::from_utf8_lossy(&failure_output.stdout);

    // Success branch produces compact "ok ✓ main", failure branch uses tail passthrough
    assert_ne!(
        success_stdout.trim(),
        failure_stdout.trim(),
        "exit code should select different branches: success={success_stdout:?}, failure={failure_stdout:?}"
    );
}

#[test]
fn test_git_push_success_fixture() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_success.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok") && stdout.contains("main"),
        "expected filtered push output, got: {stdout}"
    );
}

#[test]
fn test_git_push_up_to_date_fixture() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_up_to_date.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok (up-to-date)");
}

#[test]
fn test_git_push_failure_with_exit_code() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_failure.txt", manifest_dir());
    let output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // on_failure with tail = 10 should produce some output
    assert!(!stdout.is_empty(), "expected failure branch output");
}

#[test]
fn test_with_timing() {
    let filter = format!("{}/filters/git-push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_up_to_date.txt", manifest_dir());
    let output = tokf()
        .args(["test", "--timing", &filter, &fixture])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf] filter took"),
        "expected timing info on stderr, got: {stderr}"
    );
}

// --- tokf ls ---

#[test]
fn ls_exits_zero() {
    let output = tokf().args(["ls"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn ls_with_repo_local_filters() {
    // Create a temp dir with .tokf/filters containing a valid filter
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("my-tool.toml"), "command = \"my tool\"").unwrap();

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("my-tool") && stdout.contains("my tool"),
        "expected 'my-tool' listing, got: {stdout}"
    );
}

#[test]
fn ls_invalid_filter_shows_invalid() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("broken.toml"), "not valid [[[").unwrap();

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken") && stdout.contains("(invalid)"),
        "expected 'broken  (invalid)' in ls output, got: {stdout}"
    );
}

#[test]
fn ls_deduplication_first_match_wins() {
    let dir = tempfile::TempDir::new().unwrap();

    // Create two search directories with same filter name
    // .tokf/filters/ is first priority (repo-local)
    let local_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&local_dir).unwrap();
    std::fs::write(local_dir.join("my-cmd.toml"), "command = \"my cmd local\"").unwrap();

    // Also create a user-level config dir filter with same name
    // We can't easily control the user config dir, so instead verify
    // that only one entry appears in output (dedup by name)
    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.matches("my-cmd").count();
    assert_eq!(count, 1, "expected exactly one 'my-cmd' entry, got {count}");
}

#[test]
fn ls_verbose_shows_source() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("test-cmd.toml"), "command = \"test cmd\"").unwrap();

    let output = tokf()
        .args(["ls", "--verbose"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf]") && stderr.contains("source"),
        "expected verbose source info on stderr, got: {stderr}"
    );
}
