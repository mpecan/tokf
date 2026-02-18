use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Helper: set up a temp dir with stdlib filters copied in, and run `tokf rewrite`
/// from that directory so the filters are discoverable.
fn rewrite_with_stdlib(command: &str) -> String {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();

    // Copy stdlib filters
    let stdlib = format!("{}/filters", manifest_dir());
    for entry in std::fs::read_dir(&stdlib).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), filters_dir.join(entry.file_name())).unwrap();
    }

    let output = tokf()
        .args(["rewrite", command])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokf rewrite failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// --- Filter-derived rewrites ---

#[test]
fn rewrite_git_status() {
    let result = rewrite_with_stdlib("git status");
    assert_eq!(result, "tokf run git status");
}

#[test]
fn rewrite_git_status_with_args() {
    let result = rewrite_with_stdlib("git status --short");
    assert_eq!(result, "tokf run git status --short");
}

#[test]
fn rewrite_cargo_test() {
    let result = rewrite_with_stdlib("cargo test");
    assert_eq!(result, "tokf run cargo test");
}

#[test]
fn rewrite_cargo_test_with_args() {
    let result = rewrite_with_stdlib("cargo test --lib");
    assert_eq!(result, "tokf run cargo test --lib");
}

#[test]
fn rewrite_git_push() {
    let result = rewrite_with_stdlib("git push");
    assert_eq!(result, "tokf run git push");
}

#[test]
fn rewrite_git_diff() {
    let result = rewrite_with_stdlib("git diff");
    assert_eq!(result, "tokf run git diff");
}

#[test]
fn rewrite_git_log() {
    let result = rewrite_with_stdlib("git log");
    assert_eq!(result, "tokf run git log");
}

#[test]
fn rewrite_git_add() {
    let result = rewrite_with_stdlib("git add");
    assert_eq!(result, "tokf run git add");
}

#[test]
fn rewrite_git_commit() {
    let result = rewrite_with_stdlib("git commit");
    assert_eq!(result, "tokf run git commit");
}

// --- Built-in skip patterns ---

#[test]
fn rewrite_tokf_command_unchanged() {
    let result = rewrite_with_stdlib("tokf run git status");
    assert_eq!(result, "tokf run git status");
}

#[test]
fn rewrite_heredoc_unchanged() {
    let result = rewrite_with_stdlib("cat <<EOF");
    assert_eq!(result, "cat <<EOF");
}

// --- No matching filter ---

#[test]
fn rewrite_unknown_command_passthrough() {
    let result = rewrite_with_stdlib("unknown-cmd foo bar");
    assert_eq!(result, "unknown-cmd foo bar");
}

#[test]
fn rewrite_ls_passthrough() {
    let result = rewrite_with_stdlib("ls -la");
    assert_eq!(result, "ls -la");
}

// --- With user rewrites.toml ---

#[test]
fn rewrite_user_override() {
    let dir = tempfile::TempDir::new().unwrap();

    // Set up stdlib filters
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    // Set up user rewrites.toml with a custom rule
    std::fs::write(
        dir.path().join(".tokf/rewrites.toml"),
        r#"
[[rewrite]]
match = "^git status"
replace = "custom-wrapper {0}"
"#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "git status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "custom-wrapper git status");
}

#[test]
fn rewrite_user_skip_pattern() {
    let dir = tempfile::TempDir::new().unwrap();

    // Set up stdlib filters
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    // Set up user rewrites.toml with a skip pattern
    std::fs::write(
        dir.path().join(".tokf/rewrites.toml"),
        r#"
[skip]
patterns = ["^git status"]
"#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "git status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "git status");
}

// --- Exit code ---

#[test]
fn rewrite_always_exits_zero() {
    let output = tokf().args(["rewrite", "anything"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));
}
