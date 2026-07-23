#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

mod common;
use common::tokf;

use std::process::Stdio;

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Helper: pipe JSON to `tokf hook handle` from a fresh tempdir.
/// Embedded stdlib is always available, so no filters need to be copied.
fn hook_handle_with_stdlib(json: &str) -> (String, bool) {
    hook_handle_format_with_stdlib(json, "claude-code")
}

fn hook_handle_format_with_stdlib(json: &str, format: &str) -> (String, bool) {
    hook_handle_format_with_env(json, format, None)
}

fn hook_handle_format_with_env(
    json: &str,
    format: &str,
    env: Option<(&str, &str)>,
) -> (String, bool) {
    let dir = tempfile::TempDir::new().unwrap();

    let mut command = tokf();
    command
        .args(["hook", "handle", "--format", format])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((key, value)) = env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (stdout, output.status.success())
}

fn hook_handle_format_with_args(
    dir: &std::path::Path,
    json: &str,
    args: &[&str],
) -> (String, String, bool) {
    let mut child = tokf()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.success(),
    )
}

/// Helper: pipe JSON to `tokf hook handle` with a single custom filter.
fn hook_handle_with_filter(json: &str, filter_name: &str, filter_content: &str) -> String {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(filters_dir.join(filter_name), filter_content).unwrap();

    let mut child = tokf()
        .args(["hook", "handle"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(json.as_bytes()).unwrap();
    }

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).to_string()
}

// --- tokf hook handle ---

#[test]
fn hook_handle_rewrites_bash_git_status() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run git status"
    );
}

#[test]
fn hook_handle_rewrites_bash_with_args() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git push origin main"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run git push origin main"
    );
}

#[test]
fn hook_handle_codex_rewrites_with_updated_input() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (stdout, success) = hook_handle_format_with_env(
        json,
        "codex",
        Some(("TOKF_CODEX_REWRITE_MODE", "updated-input")),
    );
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run git status"
    );
    assert!(
        response["hookSpecificOutput"]
            .get("permissionDecisionReason")
            .is_none(),
        "Codex allow rewrite should not include a reason"
    );
}

#[test]
fn hook_handle_codex_legacy_mode_blocks_with_rerun_hint() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (stdout, success) = hook_handle_format_with_stdlib(json, "codex");
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(response["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecisionReason"],
        "Run with tokf: tokf run git status"
    );
    assert!(
        response["hookSpecificOutput"].get("updatedInput").is_none(),
        "legacy Codex mode should not emit ignored updatedInput"
    );
}

#[test]
fn hook_handle_codex_unmatched_command_silent() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-xyz-cmd-99"}}"#;
    let (stdout, success) = hook_handle_format_with_stdlib(json, "codex");
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for unmatched command, got: {stdout}"
    );
}

#[test]
fn hook_handle_no_cache_skips_project_cache_for_matching_command() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".tokf/hooks")).unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;

    let (stdout, stderr, success) = hook_handle_format_with_args(
        dir.path(),
        json,
        &["--no-cache", "hook", "handle", "--format", "codex"],
    );

    assert!(success);
    assert!(
        stderr.trim().is_empty(),
        "--no-cache hook handling should stay silent on stderr, got: {stderr}"
    );
    assert!(
        stdout.contains("Run with tokf: tokf run git status"),
        "expected conservative Codex rerun hint, got: {stdout}"
    );
}

#[test]
fn hook_handle_no_cache_does_not_enable_verbose_pipe_diagnostics() {
    // Regression for #431: --no-cache was mis-mapped into rewrite's `verbose`
    // parameter, so the hook path silently emitted `[tokf] stripped pipe …`
    // diagnostics to stderr. --no-cache must not enable verbose rewrite
    // diagnostics on stderr.
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"cargo test | grep FAILED"}}"#;

    let (stdout, stderr, success) = hook_handle_format_with_args(
        dir.path(),
        json,
        &["--no-cache", "hook", "handle", "--format", "claude-code"],
    );

    assert!(success);
    assert!(
        stderr.trim().is_empty(),
        "--no-cache must not enable verbose rewrite diagnostics on stderr, got: {stderr}"
    );
    assert!(
        stdout.contains("tokf run --baseline-pipe 'grep FAILED' cargo test"),
        "expected the pipe rewrite to still work, got: {stdout}"
    );
}

#[test]
fn hook_handle_non_bash_tool_silent() {
    let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/foo"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for non-Bash tool, got: {stdout}"
    );
}

#[test]
fn hook_handle_no_command_field_silent() {
    let json = r#"{"tool_name":"Bash","tool_input":{}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output when command is missing, got: {stdout}"
    );
}

#[test]
fn hook_handle_tokf_command_silent() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"tokf run git status"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for tokf command (skip), got: {stdout}"
    );
}

#[test]
fn hook_handle_passthroughs_compound_with_substitution_heredoc() {
    // Regression for the `git: error: switch 'm' requires a value` bug.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git add foo && git commit -m \"$(cat <<'EOF'\nfeat: x\n\nbody\nEOF\n)\" && git push"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected passthrough (empty stdout), got: {stdout}"
    );
}

#[test]
fn hook_handle_passthroughs_substitution_heredoc_with_pipe() {
    // Locks in that the substitution-heredoc skip fires *before* pipe
    // stripping. Without the skip, byte-offset slicing would mangle `-m`.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git add foo && git commit -m \"$(cat <<'EOF'\nfeat: x\nEOF\n)\" 2>&1 | tail -10"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected passthrough (empty stdout), got: {stdout}"
    );
}

#[test]
fn hook_handle_unmatched_command_silent() {
    // Use a command that has no filter in stdlib
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-xyz-cmd-99"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for unmatched command, got: {stdout}"
    );
}

#[test]
fn hook_handle_invalid_json_silent() {
    let json = "not json at all";
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for invalid JSON, got: {stdout}"
    );
}

#[test]
fn hook_handle_empty_stdin_silent() {
    let (stdout, success) = hook_handle_with_stdlib("");
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for empty stdin, got: {stdout}"
    );
}

#[test]
fn hook_handle_always_exits_zero() {
    let json = "not json";
    let (_, success) = hook_handle_with_stdlib(json);
    assert!(success, "hook handle should always exit 0");
}

#[test]
fn hook_handle_fixture_bash() {
    let fixture = format!(
        "{}/tests/fixtures/hook_pretooluse_bash.json",
        manifest_dir()
    );
    let json = std::fs::read_to_string(&fixture).unwrap();
    let (stdout, success) = hook_handle_with_stdlib(&json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run git status"
    );
}

#[test]
fn hook_handle_fixture_read() {
    let fixture = format!(
        "{}/tests/fixtures/hook_pretooluse_read.json",
        manifest_dir()
    );
    let json = std::fs::read_to_string(&fixture).unwrap();
    let (stdout, success) = hook_handle_with_stdlib(&json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for Read tool, got: {stdout}"
    );
}

// --- Pipe stripping through hook handler ---

#[test]
fn hook_handle_strips_pipe_to_grep() {
    // cargo test has a stdlib filter — pipe to grep is stripped.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"cargo test | grep FAILED"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run --baseline-pipe 'grep FAILED' cargo test"
    );
}

#[test]
fn hook_handle_preserves_pipe_to_wc() {
    // wc is not a strippable target — pipe preserved, no rewrite emitted.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status | wc -l"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for non-strippable pipe, got: {stdout}"
    );
}

#[test]
fn hook_handle_preserves_pipe_no_filter() {
    // Strippable target but no filter for the base command — pipe preserved.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-cmd | tail -5"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);
    assert!(
        stdout.trim().is_empty(),
        "expected no output for piped unknown command, got: {stdout}"
    );
}

// --- Multiple-pattern filter hook integration ---

#[test]
fn hook_handle_multiple_pattern_first_variant() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"pnpm test"}}"#;
    let stdout = hook_handle_with_filter(
        json,
        "test-runner.toml",
        r#"command = ["pnpm test", "npm test"]"#,
    );
    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run pnpm test"
    );
}

#[test]
fn hook_handle_multiple_pattern_second_variant() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"npm test"}}"#;
    let stdout = hook_handle_with_filter(
        json,
        "test-runner.toml",
        r#"command = ["pnpm test", "npm test"]"#,
    );
    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run npm test"
    );
}

#[test]
fn hook_handle_multiple_pattern_non_variant_silent() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"tokf_test_sentinel_cmd test"}}"#;
    let stdout = hook_handle_with_filter(
        json,
        "test-runner.toml",
        r#"command = ["pnpm test", "npm test"]"#,
    );
    assert!(
        stdout.trim().is_empty(),
        "expected no output for non-matching variant, got: {stdout}"
    );
}

// --- Variant hook integration tests ---

#[test]
fn hook_handle_stdlib_npm_test_rewrites() {
    // npm/test.toml from stdlib should match and rewrite "npm test"
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"npm test"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run npm test"
    );
}

#[test]
fn hook_handle_stdlib_yarn_test_rewrites() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"yarn test"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run yarn test"
    );
}

// --- --no-mask-exit-code propagation into hook rewrites (regression for #414) ---

/// Issue #414: `tokf --no-mask-exit-code hook handle` must propagate the flag
/// into the emitted `tokf run` rewrite, not silently drop it.
#[test]
fn hook_handle_no_mask_exit_code_propagates_to_rewrite() {
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (stdout, _stderr, success) =
        hook_handle_format_with_args(dir.path(), json, &["--no-mask-exit-code", "hook", "handle"]);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run --no-mask-exit-code git status"
    );
}

/// Issue #414: every member of a compound command must carry the flag.
#[test]
fn hook_handle_no_mask_exit_code_propagates_to_compound() {
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status && cargo test"}}"#;
    let (stdout, _stderr, success) =
        hook_handle_format_with_args(dir.path(), json, &["--no-mask-exit-code", "hook", "handle"]);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run --no-mask-exit-code git status && tokf run --no-mask-exit-code cargo test"
    );
}

/// Sanity: without the flag, the default masking behavior is unchanged.
#[test]
fn hook_handle_default_still_masks() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (stdout, success) = hook_handle_with_stdlib(json);
    assert!(success);

    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["updatedInput"]["command"],
        "tokf run git status"
    );
}

// --- External permission engine ask verdict (regression for #343) ---

/// Regression for #343: when the external permission engine returns an "ask"
/// verdict, the binary must exit 0 so Claude Code reads the JSON
/// `permissionDecision: "ask"` and shows the native prompt. Exit 2 would
/// short-circuit that path and turn ask into an unconditional block.
#[cfg(unix)]
#[test]
fn hook_handle_ask_verdict_exits_zero_and_emits_ask_json() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().unwrap();

    // Mock external permission engine: always returns an "ask" verdict.
    let engine = dir.path().join("engine.sh");
    std::fs::write(
        &engine,
        "#!/bin/sh\ncat >/dev/null\necho '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"needs confirmation\",\"updatedInput\":{\"command\":\"git push\"}}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Project-local rewrites.toml wires the engine in.
    let rewrites_dir = dir.path().join(".tokf");
    std::fs::create_dir_all(&rewrites_dir).unwrap();
    let rewrites = format!(
        "[permissions]\nengine = \"external\"\n\n[permissions.external]\ncommand = \"{}\"\n",
        engine.display(),
    );
    std::fs::write(rewrites_dir.join("rewrites.toml"), rewrites).unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#;
    let mut child = tokf()
        .args(["hook", "handle"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "expected exit 0 for ask verdict (issue #343), got status {:?}, stdout: {stdout}, stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let response: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        response["hookSpecificOutput"]["permissionDecision"], "ask",
        "exit 0 alone is not enough — the JSON ask verdict must reach Claude Code"
    );
}

// --- TOKF_HOOK_LOG diagnostic logging (issue #355) ---

/// Run a hook invocation with `TOKF_HOOK_LOG` set to a file, then return
/// the file's contents. The hook is invoked from a temp cwd so embedded
/// stdlib filters apply with no project config.
fn hook_handle_with_log(json: &str) -> (bool, String) {
    hook_handle_format_with_log(json, "claude-code")
}

fn hook_handle_format_with_log(json: &str, format: &str) -> (bool, String) {
    let dir = tempfile::TempDir::new().unwrap();
    let log_path = dir.path().join("hook.log");

    let mut child = tokf()
        .args(["hook", "handle", "--format", format])
        .env("TOKF_HOOK_LOG", &log_path)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let success = output.status.success();
    let log = std::fs::read_to_string(&log_path).unwrap_or_default();
    (success, log)
}

#[test]
fn hook_log_records_rewrite() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (success, log) = hook_handle_with_log(json);
    assert!(success);
    assert!(log.starts_with("---\n"), "log not YAML: {log:?}");
    assert!(log.contains("tool: Bash\n"), "missing tool field: {log:?}");
    assert!(
        log.contains("format: claude-code\n"),
        "missing format field: {log:?}"
    );
    assert!(
        log.contains("outcome: Allow\n"),
        "missing outcome field: {log:?}"
    );
    assert!(
        log.contains("before: |-\n  git status\n"),
        "missing/malformed before block: {log:?}"
    );
    assert!(
        log.contains("after: |-\n  tokf run git status\n"),
        "missing/malformed after block: {log:?}"
    );
}

#[test]
fn hook_log_records_passthrough_with_null_after() {
    // No filter for `unknown-tool`, so the hook returns PassThrough and
    // the log records `after: ~` to make the no-rewrite case unambiguous.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-tool foo"}}"#;
    let (success, log) = hook_handle_with_log(json);
    assert!(success);
    assert!(
        log.contains("outcome: PassThrough\n"),
        "expected PassThrough outcome: {log:?}"
    );
    assert!(log.contains("after: ~\n"), "expected null after: {log:?}");
}

#[test]
fn hook_log_records_codex_default_rewrite_as_deny() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let (success, log) = hook_handle_format_with_log(json, "codex");
    assert!(success);
    assert!(
        log.contains("format: codex\n"),
        "missing Codex format field: {log:?}"
    );
    assert!(
        log.contains("outcome: Deny\n"),
        "Codex default deny-rerun rewrite should log Deny outcome: {log:?}"
    );
    assert!(
        log.contains("after: |-\n  tokf run git status\n"),
        "missing/malformed after block: {log:?}"
    );
}

#[test]
fn hook_log_preserves_multiline_command_355() {
    // Regression for #355: the BEFORE block must show the original
    // newline-separated command, and the AFTER block must show the
    // rewritten command with newlines preserved between segments
    // (not glued into `head -1echo` style malformed output).
    let json =
        r#"{"tool_name":"Bash","tool_input":{"command":"git status\nls | head -1\necho hi"}}"#;
    let (success, log) = hook_handle_with_log(json);
    assert!(success);
    assert!(
        log.contains("before: |-\n  git status\n  ls | head -1\n  echo hi\n"),
        "BEFORE block not preserved verbatim: {log:?}"
    );
    assert!(
        log.contains(
            "after: |-\n  tokf run git status\n  tokf run --baseline-pipe 'head -1' ls\n  echo hi\n"
        ),
        "AFTER block does not show preserved newlines: {log:?}"
    );
    assert!(
        !log.contains("head -1echo"),
        "AFTER block contains the bug-shape malformed token: {log:?}"
    );
}

#[test]
fn hook_log_skipped_when_env_unset() {
    // Sanity: with no TOKF_HOOK_LOG env var, the hook still runs but
    // creates no log file. Avoids surprise file writes for users who
    // didn't opt into logging.
    let dir = tempfile::TempDir::new().unwrap();
    let log_path = dir.path().join("hook.log");

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let mut child = tokf()
        .args(["hook", "handle"])
        .env_remove("TOKF_HOOK_LOG")
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        !log_path.exists(),
        "hook should not create log file when env unset"
    );
}

#[test]
fn hook_log_treats_empty_env_var_as_unset() {
    // Some shells leak `TOKF_HOOK_LOG=` (empty value). Treat it as unset
    // rather than trying to open a file at the empty path.
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;

    let mut child = tokf()
        .args(["hook", "handle"])
        .env("TOKF_HOOK_LOG", "")
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "hook must not error on empty env var"
    );
}

#[test]
fn hook_log_unwritable_path_does_not_block_hook() {
    // Best-effort: an unwritable log path (parent dir doesn't exist) must
    // not block the hook from rewriting. Rewrite still emits its JSON
    // verdict on stdout; the log write is silently dropped.
    let dir = tempfile::TempDir::new().unwrap();
    let bad_log = dir.path().join("does/not/exist/hook.log");
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;

    let mut child = tokf()
        .args(["hook", "handle"])
        .env("TOKF_HOOK_LOG", &bad_log)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "hook should not error: {stdout}");
    assert!(
        stdout.contains("tokf run git status"),
        "rewrite must still happen even when logging fails: {stdout}"
    );
    assert!(
        !bad_log.exists(),
        "log file must not appear at unwritable path"
    );
}

#[test]
fn hook_log_records_ask_outcome() {
    // Wires the same external-engine harness as the existing #343 test
    // but with TOKF_HOOK_LOG set. Confirms the Ask outcome flows through
    // the single log call site at the bottom of process_command.
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::TempDir::new().unwrap();
    let log_path = dir.path().join("hook.log");

    let engine = dir.path().join("engine.sh");
    std::fs::write(
        &engine,
        "#!/bin/sh\ncat >/dev/null\necho '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"ask\",\"permissionDecisionReason\":\"please confirm\",\"updatedInput\":{\"command\":\"git push\"}}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();

    let rewrites_dir = dir.path().join(".tokf");
    std::fs::create_dir_all(&rewrites_dir).unwrap();
    let rewrites = format!(
        "[permissions]\nengine = \"external\"\n\n[permissions.external]\ncommand = \"{}\"\n",
        engine.display(),
    );
    std::fs::write(rewrites_dir.join("rewrites.toml"), rewrites).unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#;
    let mut child = tokf()
        .args(["hook", "handle"])
        .env("TOKF_HOOK_LOG", &log_path)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(json.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("outcome: Ask\n"),
        "expected Ask outcome in log: {log}"
    );
    assert!(
        log.contains("before: |-\n  git push\n"),
        "missing/malformed before block: {log}"
    );
}
