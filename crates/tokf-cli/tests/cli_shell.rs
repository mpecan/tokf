#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

const fn tokf_path() -> &'static str {
    env!("CARGO_BIN_EXE_tokf")
}

// --- shell mode entry ---

#[test]
fn shell_c_true_exits_zero() {
    let output = tokf().args(["-c", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_c_false_exits_nonzero() {
    let output = tokf().args(["-c", "false"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn shell_c_echo_produces_output() {
    let output = tokf().args(["-c", "echo hello"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn shell_c_missing_command_arg() {
    let output = tokf().arg("-c").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("shell mode requires a command argument"),
        "expected error message, got: {stderr}"
    );
}

// --- combined flags ---

#[test]
fn shell_cu_works() {
    let output = tokf().args(["-cu", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_ec_works() {
    let output = tokf().args(["-ec", "true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_ecu_works() {
    let output = tokf().args(["-ecu", "true"]).output().unwrap();
    assert!(output.status.success());
}

// --- compound delegation ---

#[test]
fn shell_compound_and_delegates() {
    let output = tokf().args(["-c", "true && true"]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_compound_failure_propagates() {
    let output = tokf().args(["-c", "false && true"]).output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn shell_pipe_delegates_to_sh() {
    let output = tokf().args(["-c", "echo hello | cat"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn shell_redirect_delegates_to_sh() {
    // Redirections should be handled by the real shell.
    let output = tokf()
        .args(["-c", "echo hello > /dev/null"])
        .output()
        .unwrap();
    assert!(output.status.success());
    // Output was redirected to /dev/null, so stdout should be empty.
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

// --- exit code preservation ---

#[test]
fn shell_exit_code_42() {
    let output = tokf().args(["-c", "exit 42"]).output().unwrap();
    assert_eq!(output.status.code(), Some(42));
}

// --- empty and whitespace commands ---

#[test]
fn shell_empty_command() {
    let output = tokf().args(["-c", ""]).output().unwrap();
    assert!(output.status.success());
}

#[test]
fn shell_whitespace_only_command() {
    let output = tokf().args(["-c", "   "]).output().unwrap();
    // Whitespace-only delegates to sh, which treats it as a no-op.
    assert!(output.status.success());
}

// --- double-wrap prevention ---

#[test]
fn shell_tokf_run_not_double_wrapped() {
    // When shell mode receives a command that is already wrapped with
    // `tokf run`, it should delegate to sh (the `^tokf ` skip pattern
    // prevents filter matching). Verify the inner tokf actually runs.
    let inner_cmd = format!("{} run echo not-double-wrapped", env!("CARGO_BIN_EXE_tokf"));
    let output = tokf().args(["-c", &inner_cmd]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("not-double-wrapped"),
        "expected inner tokf to produce output, got: {stdout}"
    );
}

// --- argv mode (multiple args after -c) ---

#[test]
fn shell_argv_mode_simple() {
    let output = tokf().args(["-c", "echo", "hello"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
}

#[test]
fn shell_argv_mode_multiple_args() {
    let output = tokf()
        .args(["-c", "echo", "hello", "world"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello world"
    );
}

#[test]
fn shell_argv_mode_preserves_spaces_in_args() {
    let output = tokf().args(["-c", "echo", "hello world"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello world"
    );
}

#[test]
fn shell_argv_mode_special_chars() {
    // Single quotes, dollar signs, backticks should all be preserved as literals.
    let output = tokf()
        .args(["-c", "echo", "it's $HOME `whoami`"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("it's $HOME `whoami`"),
        "special chars should be literal, got: {stdout}"
    );
}

#[test]
fn shell_argv_mode_exit_code() {
    let output = tokf().args(["-c", "false"]).output().unwrap();
    assert!(!output.status.success());
}

// --- argv mode: double-dash and flag passthrough ---

#[test]
fn shell_argv_mode_double_dash_passthrough() {
    // `tokf -c echo -- hello` should preserve `--` and `hello`.
    let output = tokf().args(["-c", "echo", "--", "hello"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("-- hello") || stdout.contains("hello"),
        "expected -- and hello in output, got: {stdout}"
    );
}

#[test]
fn shell_argv_mode_flags_after_separator() {
    // `tokf -c echo -- --flag` should preserve `--flag` literally.
    let output = tokf()
        .args(["-c", "echo", "--", "--flag"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--flag"),
        "expected --flag in output, got: {stdout}"
    );
}

#[test]
fn shell_argv_mode_triggers_filtering() {
    // With TOKF_VERBOSE, a command that has a filter (git status) should show
    // "rewritten to" in stderr, proving the argv mode fix works.
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "git", "status", "--short"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected 'rewritten to' in stderr (filter should match), got: {stderr}"
    );
}

#[test]
fn shell_argv_mode_no_filter_delegates_safely() {
    // A command with no filter should show "no filter match" and still work.
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "echo", "hello", "world"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no filter match"),
        "expected 'no filter match' in stderr, got: {stderr}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello world"
    );
}

// --- argv mode: argument boundary preservation ---

#[test]
fn shell_argv_mode_preserves_arg_boundaries_through_filter() {
    // When a filter matches in argv mode, the rewritten command must preserve
    // argument boundaries. Verify by checking that the verbose "rewritten to"
    // output contains quoted args (e.g. 'git' 'status' '--short').
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "git", "status", "--short"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected filter match, got: {stderr}"
    );
    // The rewritten command should contain quoted args, not bare args.
    assert!(
        stderr.contains("'git'") && stderr.contains("'status'"),
        "expected quoted args in rewritten command for boundary safety, got: {stderr}"
    );
}

#[test]
fn shell_argv_mode_multiword_arg_stays_single_token() {
    // An argument containing spaces (e.g. a commit message) must remain a
    // single quoted token in the rewritten command, not be split by the shell.
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "git", "log", "--format=hello world"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected filter match, got: {stderr}"
    );
    // The multi-word arg must appear as a single quoted token, proving
    // argument boundaries survive the rewrite.
    assert!(
        stderr.contains("'--format=hello world'"),
        "expected multi-word arg as single quoted token, got: {stderr}"
    );
}

// --- environment variable controls ---

#[test]
fn shell_no_filter_env_delegates() {
    let output = tokf()
        .env("TOKF_NO_FILTER", "1")
        .args(["-c", "echo bypassed"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "bypassed");
}

#[test]
fn shell_argv_mode_no_filter_env_delegates() {
    let output = tokf()
        .env("TOKF_NO_FILTER", "1")
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "echo", "bypassed"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("TOKF_NO_FILTER set"),
        "expected TOKF_NO_FILTER message in stderr, got: {stderr}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "bypassed");
}

#[test]
fn shell_verbose_env_prints_to_stderr() {
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "true"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[tokf]"),
        "expected verbose output on stderr, got: {stderr}"
    );
}

// --- shell mode filter matching (the #277 bug) ---

#[test]
fn shell_c_filters_git_status_with_interleaved_flags() {
    // The #277 bug: `git -C /path status` was not matched by shell mode
    // because the old regex-based matching didn't handle interleaved flags.
    let dir = TempDir::new().unwrap();

    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Use git -C <dir> status — interleaved flag between "git" and "status".
    let cmd = format!("git -C {} status", dir.path().display());
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", &cmd])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected `git -C <dir> status` to match filter, stderr: {stderr}"
    );
}

#[test]
fn shell_c_filters_git_no_pager_log() {
    // Another interleaved-flag case: `git --no-pager log`
    let dir = TempDir::new().unwrap();

    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Create an initial commit so `git log` has output.
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", "git --no-pager log --oneline"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected `git --no-pager log` to match filter, stderr: {stderr}"
    );
}

#[test]
fn shell_c_filters_full_path_git() {
    // Full-path invocation: /usr/bin/git status should match "git status".
    let git_path = Command::new("which")
        .arg("git")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if git_path.is_empty() {
        eprintln!("skipping: git not found");
        return;
    }

    let dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let cmd = format!("{git_path} status");
    let output = tokf()
        .env("TOKF_VERBOSE", "1")
        .args(["-c", &cmd])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to"),
        "expected full-path `{git_path} status` to match filter, stderr: {stderr}"
    );
}

// --- make integration (SHELL override) ---

#[test]
fn make_shell_override_filters_git_status() {
    let dir = TempDir::new().unwrap();

    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Makefile with multiple recipes to exercise compound commands and
    // interleaved flags — the exact scenario that broke in #277.
    let makefile = format!(
        "SHELL := {tokf}\n\
         .PHONY: check status\n\
         check: status\n\
         \tgit -C {dir} log --oneline -1 2>/dev/null || true\n\
         status:\n\
         \tgit -C {dir} status\n",
        tokf = tokf_path(),
        dir = dir.path().display()
    );
    std::fs::write(dir.path().join("Makefile"), makefile).unwrap();

    // Create a commit so `git log` has output.
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let output = Command::new("make")
        .arg("check")
        .current_dir(dir.path())
        .env("TOKF_VERBOSE", "1")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to") || stderr.contains("[tokf]"),
        "expected tokf to process git commands via SHELL override, stderr: {stderr}"
    );
}

// --- just integration (--shell override) ---

#[test]
fn just_shell_override_filters_git_status() {
    if Command::new("just").arg("--version").output().is_err() {
        eprintln!("skipping: `just` not found in PATH");
        return;
    }

    let dir = TempDir::new().unwrap();

    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Justfile with interleaved flags — exercises the #277 fix.
    let justfile = format!(
        "set shell := [\"{tokf}\", \"-cu\"]\n\
         \n\
         check: status\n\
         \tgit -C {dir} log --oneline -1\n\
         \n\
         status:\n\
         \tgit -C {dir} status\n",
        tokf = tokf_path(),
        dir = dir.path().display()
    );
    std::fs::write(dir.path().join("justfile"), justfile).unwrap();

    let output = Command::new("just")
        .arg("check")
        .current_dir(dir.path())
        .env("TOKF_VERBOSE", "1")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rewritten to") || stderr.contains("[tokf]"),
        "expected tokf to process git commands via just shell override, stderr: {stderr}"
    );
}
