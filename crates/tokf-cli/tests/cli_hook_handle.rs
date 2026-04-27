#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::{Command, Stdio};

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

fn manifest_dir() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

/// Helper: pipe JSON to `tokf hook handle` from a fresh tempdir.
/// Embedded stdlib is always available, so no filters need to be copied.
fn hook_handle_with_stdlib(json: &str) -> (String, bool) {
    let dir = tempfile::TempDir::new().unwrap();

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
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    (stdout, output.status.success())
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
