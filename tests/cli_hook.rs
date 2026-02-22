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
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"yarn test"}}"#;
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

// --- tokf hook install ---

#[test]
fn hook_install_creates_files() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check hook script was created
    let hook_script = dir.path().join(".tokf/hooks/pre-tool-use.sh");
    assert!(hook_script.exists(), "hook script should exist");

    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(content.starts_with("#!/bin/sh\n"));
    assert!(content.contains("hook handle"));

    // Check settings.json was created
    let settings = dir.path().join(".claude/settings.json");
    assert!(settings.exists(), "settings.json should exist");

    let settings_content = std::fs::read_to_string(&settings).unwrap();
    let value: serde_json::Value = serde_json::from_str(&settings_content).unwrap();
    assert!(value["hooks"]["PreToolUse"].is_array());

    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["matcher"], "Bash");
}

#[test]
fn hook_install_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();

    // Install twice
    for _ in 0..2 {
        let output = tokf()
            .args(["hook", "install"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    let settings = dir.path().join(".claude/settings.json");
    let content = std::fs::read_to_string(&settings).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        1,
        "should have exactly one entry after double install"
    );
}

#[test]
fn hook_install_preserves_existing_settings() {
    let dir = tempfile::TempDir::new().unwrap();

    // Create existing settings.json with custom content
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{"permissions": {"allow": ["Read"]}, "hooks": {"PostToolUse": []}}"#,
    )
    .unwrap();

    let output = tokf()
        .args(["hook", "install"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let content = std::fs::read_to_string(claude_dir.join("settings.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Existing keys preserved
    assert!(value["permissions"]["allow"].is_array());
    assert!(value["hooks"]["PostToolUse"].is_array());
    // Hook added
    assert!(value["hooks"]["PreToolUse"].is_array());
}

#[test]
fn hook_install_shows_info_on_stderr() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hook installed"),
        "expected install confirmation, got: {stderr}"
    );
    assert!(
        stderr.contains("script:"),
        "expected script path, got: {stderr}"
    );
    assert!(
        stderr.contains("settings:"),
        "expected settings path, got: {stderr}"
    );
}
