use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// Helper: run `tokf rewrite` from a fresh tempdir.
/// Embedded stdlib is always available, so no filters need to be copied.
fn rewrite_with_stdlib(command: &str) -> String {
    let dir = tempfile::TempDir::new().unwrap();

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

#[test]
fn rewrite_cargo_build() {
    let result = rewrite_with_stdlib("cargo build");
    assert_eq!(result, "tokf run cargo build");
}

#[test]
fn rewrite_cargo_clippy() {
    let result = rewrite_with_stdlib("cargo clippy");
    assert_eq!(result, "tokf run cargo clippy");
}

#[test]
fn rewrite_ls() {
    let result = rewrite_with_stdlib("ls -la");
    assert_eq!(result, "tokf run ls -la");
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

// --- CommandPattern::Multiple (both patterns rewrite) ---

#[test]
fn rewrite_multiple_patterns_first_variant() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "pnpm test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "tokf run pnpm test"
    );
}

#[test]
fn rewrite_multiple_patterns_second_variant() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "npm test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "tokf run npm test"
    );
}

#[test]
fn rewrite_multiple_patterns_non_variant_passthrough() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "yarn test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "yarn test");
}

// --- Wildcard pattern rewrites ---

#[test]
fn rewrite_wildcard_pattern_matches_any_subcommand() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("npm-run.toml"), r#"command = "npm run *""#).unwrap();

    let output = tokf()
        .args(["rewrite", "npm run build"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "tokf run npm run build"
    );
}

#[test]
fn rewrite_wildcard_pattern_no_match_without_wildcard_arg() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join("npm-run.toml"), r#"command = "npm run *""#).unwrap();

    // "npm run" without a subcommand should NOT match the wildcard pattern
    let output = tokf()
        .args(["rewrite", "npm run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "npm run");
}

// --- Golangci-lint disambiguation ---

#[test]
fn rewrite_golangci_lint_run_matches() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("golangci-lint.toml"),
        r#"command = "golangci-lint run""#,
    )
    .unwrap();

    let output = tokf()
        .args(["rewrite", "golangci-lint run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "tokf run golangci-lint run"
    );
}

#[test]
fn rewrite_golangci_lint_alone_passthrough() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("golangci-lint.toml"),
        r#"command = "golangci-lint run""#,
    )
    .unwrap();

    // Bare "golangci-lint" should NOT match "golangci-lint run"
    let output = tokf()
        .args(["rewrite", "golangci-lint"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "golangci-lint"
    );
}

// --- Pipe stripping (simple pipes stripped when filter matches) ---

#[test]
fn rewrite_pipe_grep_stripped() {
    // cargo/test filter matches — pipe to grep is stripped.
    let result = rewrite_with_stdlib("cargo test | grep FAILED");
    assert_eq!(result, "tokf run cargo test");
}

#[test]
fn rewrite_pipe_head_stripped() {
    let result = rewrite_with_stdlib("git diff HEAD | head -5");
    assert_eq!(result, "tokf run git diff HEAD");
}

#[test]
fn rewrite_pipe_tail_stripped() {
    let result = rewrite_with_stdlib("cargo test | tail -n 5");
    assert_eq!(result, "tokf run cargo test");
}

#[test]
fn rewrite_pipe_grep_pattern_stripped() {
    let result = rewrite_with_stdlib("git status | grep modified");
    assert_eq!(result, "tokf run git status");
}

// --- Pipes that are NOT stripped ---

#[test]
fn rewrite_pipe_tail_follow_passes_through() {
    let result = rewrite_with_stdlib("cargo test | tail -f");
    assert_eq!(result, "cargo test | tail -f");
}

#[test]
fn rewrite_pipe_wc_passes_through() {
    let result = rewrite_with_stdlib("git status | wc -l");
    assert_eq!(result, "git status | wc -l");
}

#[test]
fn rewrite_pipe_no_filter_preserves_pipe() {
    let result = rewrite_with_stdlib("unknown-cmd | tail -5");
    assert_eq!(result, "unknown-cmd | tail -5");
}

#[test]
fn rewrite_multi_pipe_chain_passes_through() {
    let result = rewrite_with_stdlib("git status | grep M | wc -l");
    assert_eq!(result, "git status | grep M | wc -l");
}

#[test]
fn rewrite_multi_pipe_with_tail_passes_through() {
    let result = rewrite_with_stdlib("cargo test | grep x | tail -5");
    assert_eq!(result, "cargo test | grep x | tail -5");
}

// --- Compound + pipe ---

#[test]
fn rewrite_compound_then_tail_stripped() {
    let result = rewrite_with_stdlib("git add . && cargo test | tail -5");
    assert_eq!(result, "tokf run git add . && tokf run cargo test");
}

#[test]
fn rewrite_logical_or_still_rewritten_integration() {
    let result = rewrite_with_stdlib("cargo test || echo failed");
    assert_eq!(result, "tokf run cargo test || echo failed");
}

#[test]
fn rewrite_logical_or_then_pipe_stripped() {
    // Mixed: || followed by |. The compound splitter handles each segment independently.
    // Second segment "cargo test | grep ok" gets pipe stripped because cargo test has a filter.
    let result = rewrite_with_stdlib("cargo test || cargo test | grep ok");
    assert_eq!(result, "tokf run cargo test || tokf run cargo test");
}

#[test]
fn rewrite_quoted_pipe_not_treated_as_pipe() {
    // A pipe inside single quotes is not a shell pipe operator — the command should be rewritten.
    // git/log has a stdlib filter and the | is inside quotes so no bare pipe is detected.
    let result = rewrite_with_stdlib("git log --grep='feat|fix'");
    assert_eq!(result, "tokf run git log --grep='feat|fix'");
}

// --- Additional pipe edge cases ---

#[test]
fn rewrite_pipe_head_bytes_passes_through() {
    // head -c (byte mode) is not strippable — different semantic.
    let result = rewrite_with_stdlib("cargo test | head -c 50");
    assert_eq!(result, "cargo test | head -c 50");
}

#[test]
fn rewrite_compound_non_strippable_pipe_passes_through() {
    // First segment rewritten, second has a non-strippable pipe (wc) — preserved.
    let result = rewrite_with_stdlib("git add . && cargo test | wc -l");
    assert_eq!(result, "tokf run git add . && cargo test | wc -l");
}

#[test]
fn rewrite_compound_pipe_no_filter_preserves_pipe() {
    // First segment rewritten, second has a strippable pipe but no filter — preserved.
    let result = rewrite_with_stdlib("git add . && unknown-cmd | tail -5");
    assert_eq!(result, "tokf run git add . && unknown-cmd | tail -5");
}

#[test]
fn rewrite_semicolon_compound() {
    let result = rewrite_with_stdlib("git add .; git status");
    assert_eq!(result, "tokf run git add .; tokf run git status");
}

// --- Exit code ---

#[test]
fn rewrite_always_exits_zero() {
    let output = tokf().args(["rewrite", "anything"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.status.code(), Some(0));
}
