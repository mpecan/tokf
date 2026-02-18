use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Recursively copy all files from `src` into `dst`, creating subdirectories as needed.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
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
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
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
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
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
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
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

    assert_ne!(
        success_stdout.trim(),
        failure_stdout.trim(),
        "exit code should select different branches: success={success_stdout:?}, failure={failure_stdout:?}"
    );
}

#[test]
fn test_git_push_success_fixture() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
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
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_up_to_date.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok (up-to-date)");
}

#[test]
fn test_git_push_failure_with_exit_code() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/tests/fixtures/git_push_failure.txt", manifest_dir());
    let output = tokf()
        .args(["test", &filter, &fixture, "--exit-code", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "expected failure branch output");
}

#[test]
fn test_with_timing() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
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
fn ls_stdlib_contains_all_expected_filters() {
    // Copy stdlib filters (nested) into a repo-local .tokf/filters/ so the test
    // is self-contained (the test binary lives in target/debug/, not the project root).
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");

    let stdlib = format!("{}/filters", manifest_dir());
    copy_dir_recursive(std::path::Path::new(&stdlib), &filters_dir);

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // With nested structure, commands are shown (not filenames)
    for cmd in [
        "git push",
        "git add",
        "git commit",
        "git diff",
        "git log",
        "git status",
        "cargo test",
    ] {
        assert!(
            stdout.contains(cmd),
            "expected command '{cmd}' in ls output, got: {stdout}"
        );
    }
}

#[test]
fn ls_with_repo_local_filters() {
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
fn ls_nested_filter_shows_relative_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let git_dir = dir.path().join(".tokf/filters/git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("push.toml"), "command = \"git push\"").unwrap();

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show the relative path "git/push" and command "git push"
    assert!(
        stdout.contains("git/push") && stdout.contains("git push"),
        "expected 'git/push → git push' in ls output, got: {stdout}"
    );
}

#[test]
fn ls_deduplication_first_match_wins() {
    let dir = tempfile::TempDir::new().unwrap();
    let local_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&local_dir).unwrap();
    std::fs::write(local_dir.join("my-cmd.toml"), "command = \"my cmd local\"").unwrap();

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

// --- tokf which ---

#[test]
fn which_git_push_finds_stdlib() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    copy_dir_recursive(
        std::path::Path::new(&format!("{}/filters", manifest_dir())),
        &filters_dir,
    );

    let output = tokf()
        .args(["which", "git push"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git/push") && stdout.contains("git push"),
        "expected 'git/push' and 'git push' in which output, got: {stdout}"
    );
}

#[test]
fn which_git_push_with_trailing_args() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    copy_dir_recursive(
        std::path::Path::new(&format!("{}/filters", manifest_dir())),
        &filters_dir,
    );

    let output = tokf()
        .args(["which", "git push origin main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git/push"),
        "expected 'git/push' in which output, got: {stdout}"
    );
}

#[test]
fn which_unknown_command_exits_one() {
    let output = tokf()
        .args(["which", "unknown-cmd-xyz-99"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no filter found"),
        "expected 'no filter found' in stderr, got: {stderr}"
    );
}

#[test]
fn which_shows_priority_label() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("my-tool.toml"), "command = \"my tool\"").unwrap();

    let output = tokf()
        .args(["which", "my tool"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A filter in .tokf/filters is always [local]
    assert!(
        stdout.contains("[local]"),
        "expected [local] priority label in which output, got: {stdout}"
    );
}

#[test]
fn ls_verbose_shows_all_patterns_for_multiple() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let output = tokf()
        .args(["ls", "--verbose"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pnpm test") && stderr.contains("npm test"),
        "expected both patterns in verbose output, got: {stderr}"
    );
}

#[test]
fn ls_skips_invalid_toml_silently() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("bad.toml"), "not valid toml [[[").unwrap();
    std::fs::write(filters_dir.join("good.toml"), "command = \"good cmd\"").unwrap();

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("good cmd"),
        "expected valid filter to appear, got: {stdout}"
    );
    assert!(
        !stdout.contains("bad"),
        "invalid filter should be silently skipped, got: {stdout}"
    );
}

#[test]
fn which_skips_invalid_toml_silently() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("bad.toml"), "not valid toml [[[").unwrap();
    std::fs::write(filters_dir.join("good.toml"), "command = \"good cmd\"").unwrap();

    let output = tokf()
        .args(["which", "good cmd"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("good cmd"),
        "expected valid filter to be found, got: {stdout}"
    );
}
