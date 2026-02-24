#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

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
    assert!(
        stderr.is_empty() || stderr.contains("[tokf]"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn run_false_propagates_exit_code() {
    let output = tokf()
        .args(["run", "--no-mask-exit-code", "false"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(0));
}

#[test]
fn run_exit_code_42() {
    let output = tokf()
        .args(["run", "--no-mask-exit-code", "sh", "-c", "exit 42"])
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
        .args([
            "run",
            "--no-mask-exit-code",
            "--no-filter",
            "sh",
            "-c",
            "exit 7",
        ])
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
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());
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
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());

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
    let fixture = format!("{}/filters/git/push_test/success.txt", manifest_dir());
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
    let fixture = format!("{}/filters/git/push_test/up_to_date.txt", manifest_dir());
    let output = tokf().args(["test", &filter, &fixture]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "ok (up-to-date)");
}

#[test]
fn test_git_push_failure_with_exit_code() {
    let filter = format!("{}/filters/git/push.toml", manifest_dir());
    let fixture = format!("{}/filters/git/push_test/failure.txt", manifest_dir());
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
    let fixture = format!("{}/filters/git/push_test/up_to_date.txt", manifest_dir());
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
    // Embedded stdlib is always available — no need to copy filters
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["ls"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    for cmd in [
        "git push",
        "git add",
        "git commit",
        "git diff",
        "git log",
        "git status",
        "cargo test",
        "cargo build",
        "cargo clippy",
        "ls",
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
    // Embedded stdlib is always available — no need to copy filters
    let dir = tempfile::TempDir::new().unwrap();

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
    // Embedded stdlib filter shows [built-in] when no local override
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["which", "git push"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[built-in]"),
        "expected [built-in] priority label in which output, got: {stdout}"
    );
}

#[test]
fn which_shows_local_label_for_local_filter() {
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
    assert!(
        stdout.contains("[local]"),
        "expected [local] priority label for local filter, got: {stdout}"
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

// --- tokf show ---

#[test]
fn show_hash_produces_64_char_lowercase_hex() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push", "--hash"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout.trim();
    assert_eq!(hash.len(), 64, "hash must be 64 chars, got: {hash:?}");
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "hash must be lowercase hex, got: {hash:?}"
    );
}

#[test]
fn show_hash_is_stable_across_two_calls() {
    let dir = tempfile::TempDir::new().unwrap();
    let hash1 = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    let hash2 = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_eq!(hash1, hash2, "hash must be identical across invocations");
}

#[test]
fn show_hash_differs_for_different_filters() {
    let dir = tempfile::TempDir::new().unwrap();
    let hash_push = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    let hash_build = String::from_utf8_lossy(
        &tokf()
            .args(["show", "cargo/build", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_ne!(
        hash_push, hash_build,
        "different filters must produce different hashes"
    );
}

#[test]
fn show_hash_nonexistent_filter_exits_one() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "no/such/filter", "--hash"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn show_git_push_prints_toml() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git push"),
        "expected TOML with 'git push' command, got: {stdout}"
    );
    assert!(
        stdout.contains("on_success") || stdout.contains("on_failure"),
        "expected TOML content, got: {stdout}"
    );
}

#[test]
fn show_with_toml_extension_works() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push.toml"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git push"),
        "expected TOML content with .toml extension variant, got: {stdout}"
    );
}

#[test]
fn show_nonexistent_exits_one() {
    let output = tokf().args(["show", "no/such/filter"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("filter not found"),
        "expected 'filter not found' in stderr, got: {stderr}"
    );
}

#[test]
fn show_local_filter_prints_disk_content() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("my-tool.toml"),
        "command = \"my tool\"\n# local comment\n",
    )
    .unwrap();

    let output = tokf()
        .args(["show", "my-tool"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("local comment"),
        "expected local filter content, got: {stdout}"
    );
}

#[test]
fn show_cargo_build_nested_embedded_path() {
    // Verifies that show works for nested paths (cargo/build) in the embedded stdlib
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "cargo/build"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokf show cargo/build should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo build"),
        "expected TOML with 'cargo build' command, got: {stdout}"
    );
    assert!(
        stdout.contains("on_success") || stdout.contains("skip"),
        "expected TOML content with on_success or skip, got: {stdout}"
    );
}

#[test]
fn run_embedded_filter_from_empty_dir() {
    // From a directory with no local .tokf/filters, the embedded stdlib should still be active.
    // Use `--verbose` to confirm the built-in filter was matched.
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["--verbose", "run", "git", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // git status may succeed or fail depending on whether dir is a git repo; either is fine.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("built-in") || stderr.contains("git/status"),
        "expected verbose output indicating built-in filter was matched, got: {stderr}"
    );
}

#[test]
fn ls_verbose_shows_builtin_for_embedded_filter() {
    // From a dir with no local filters, embedded stdlib filters should show source as <built-in>
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["ls", "--verbose"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("<built-in>"),
        "expected '<built-in>' in verbose ls output for embedded filters, got: {stderr}"
    );
}

// --- exit-code masking (default on): always exit 0, prepend error line (claude-code#27621) ---

#[test]
fn mask_exit_code_returns_zero_on_failure() {
    let output = tokf()
        .args(["run", "sh", "-c", "echo failure-msg; exit 1"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("failure-msg"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 1\n"),
        "error line should be prepended, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_no_error_line_on_success() {
    let output = tokf()
        .args(["run", "echo", "success-msg"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("success-msg"),
        "successful command should print to stdout, got: {stdout}"
    );
    assert!(
        !stdout.contains("Error: Exit code"),
        "successful command should not have error line, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_filtered_failure() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("sh.toml"),
        "command = \"sh\"\n[on_failure]\noutput = \"FILTERED_FAIL\"",
    )
    .unwrap();

    let output = tokf()
        .args(["run", "sh", "-c", "echo raw; exit 1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FILTERED_FAIL"),
        "filtered output should be present, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 1\n"),
        "error line should be prepended, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_signal_exit_code() {
    let output = tokf()
        .args(["run", "sh", "-c", "echo signal-msg; exit 130"])
        .output()
        .unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("signal-msg"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        stdout.starts_with("Error: Exit code 130\n"),
        "should prepend signal exit code, got: {stdout}"
    );
}

#[test]
fn mask_exit_code_empty_output_on_failure() {
    let output = tokf().args(["run", "sh", "-c", "exit 1"]).output().unwrap();
    assert!(output.status.success(), "should exit 0 by default");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "Error: Exit code 1",
        "empty failure should only show exit code line, got: {stdout}"
    );
}

#[test]
fn no_mask_exit_code_propagates_exit_code() {
    let output = tokf()
        .args([
            "run",
            "--no-mask-exit-code",
            "sh",
            "-c",
            "echo hello; exit 42",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(42));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "output should contain command output, got: {stdout}"
    );
    assert!(
        !stdout.contains("Error: Exit code"),
        "should not have error line with --no-mask-exit-code, got: {stdout}"
    );
}
