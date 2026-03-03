#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
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
