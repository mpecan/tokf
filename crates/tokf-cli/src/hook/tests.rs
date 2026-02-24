use super::*;

// --- handle_json ---

#[test]
fn handle_bash_with_no_matching_filter() {
    // No filters in search path, so no rewrite should happen
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-cmd"}}"#;
    assert!(!handle_json(json));
}

#[test]
fn handle_non_bash_tool_passes_through() {
    let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/foo"}}"#;
    assert!(!handle_json(json));
}

#[test]
fn handle_bash_no_command_passes_through() {
    let json = r#"{"tool_name":"Bash","tool_input":{}}"#;
    assert!(!handle_json(json));
}

#[test]
fn handle_invalid_json_passes_through() {
    assert!(!handle_json("not json"));
}

#[test]
fn handle_empty_input_passes_through() {
    assert!(!handle_json(""));
}

#[test]
fn handle_tokf_command_not_rewritten() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"tokf run git status"}}"#;
    assert!(!handle_json(json));
}

// --- handle_json_with_config (fix #9: test the rewrite path) ---

#[test]
fn handle_json_with_config_rewrites_matching_command() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_config(json, &config, &[dir.path().to_path_buf()]);
    assert!(result, "expected rewrite to occur for matching command");
}

#[test]
fn handle_json_with_config_no_match_returns_false() {
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-xyz-cmd-99"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_config(json, &config, &[dir.path().to_path_buf()]);
    assert!(!result);
}

#[test]
fn handle_json_rewrites_single_env_var_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"DEBUG=1 git status"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_config(json, &config, &[dir.path().to_path_buf()]);
    assert!(result, "expected rewrite for env-prefixed matching command");
}

#[test]
fn handle_json_rewrites_multiple_env_vars_prefix() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let json =
        r#"{"tool_name":"Bash","tool_input":{"command":"RUST_LOG=debug TERM=xterm cargo test"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_config(json, &config, &[dir.path().to_path_buf()]);
    assert!(result, "expected rewrite for multiple env vars prefix");
}

#[test]
fn handle_json_rewrites_env_var_with_strippable_pipe() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"RUST_LOG=debug cargo test | grep FAILED"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_config(json, &config, &[dir.path().to_path_buf()]);
    assert!(result, "expected rewrite for env var + strippable pipe");
}

#[test]
fn handle_json_env_prefixed_tokf_command_not_rewritten() {
    // DEBUG=1 tokf run ... must not be rewritten — same as tokf run ... passthrough.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"DEBUG=1 tokf run git status"}}"#;
    assert!(!handle_json(json));
}

// --- patch_settings ---

#[test]
fn patch_creates_new_settings_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings = dir.path().join(".claude/settings.json");
    let hook = dir.path().join("hook.sh");

    patch_settings(&settings, &hook).unwrap();

    let content = std::fs::read_to_string(&settings).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    let pre_tool = &value["hooks"]["PreToolUse"];
    assert!(pre_tool.is_array());
    assert_eq!(pre_tool.as_array().unwrap().len(), 1);
    assert_eq!(pre_tool[0]["matcher"], "Bash");
}

#[test]
fn patch_preserves_existing_settings() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings_path = dir.path().join("settings.json");
    let hook = dir.path().join("hook.sh");

    std::fs::write(
        &settings_path,
        r#"{"customKey": "customValue", "hooks": {"PostToolUse": []}}"#,
    )
    .unwrap();

    patch_settings(&settings_path, &hook).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(value["customKey"], "customValue");
    assert!(value["hooks"]["PostToolUse"].is_array());
    assert!(value["hooks"]["PreToolUse"].is_array());
}

#[test]
fn patch_idempotent_install() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings_path = dir.path().join("settings.json");
    let hook = dir.path().join("tokf-hook.sh");

    // Install twice
    patch_settings(&settings_path, &hook).unwrap();
    patch_settings(&settings_path, &hook).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        1,
        "should have exactly one hook entry after double install"
    );
}

#[test]
fn patch_preserves_non_tokf_hooks() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings_path = dir.path().join("settings.json");
    let hook = dir.path().join("tokf-hook.sh");

    std::fs::write(
        &settings_path,
        r#"{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "/other/tool.sh" }]
      }
    ]
  }
}"#,
    )
    .unwrap();

    patch_settings(&settings_path, &hook).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        2,
        "should have both the existing hook and the new tokf hook"
    );
}

#[test]
fn patch_settings_quotes_path_with_spaces() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings_path = dir.path().join("settings.json");
    // Simulate a hook script path that contains spaces
    let hook = std::path::Path::new("/Users/my name/.tokf/hooks/pre-tool-use.sh");

    patch_settings(&settings_path, hook).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    let cmd = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(
        cmd.starts_with('\''),
        "command should be single-quoted for shell safety, got: {cmd}"
    );
    assert!(
        cmd.contains("my name"),
        "path with space should be preserved, got: {cmd}"
    );
}

#[test]
fn patch_fails_on_corrupt_settings_json() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings_path = dir.path().join("settings.json");
    let hook = dir.path().join("hook.sh");

    std::fs::write(&settings_path, "not valid json {{{").unwrap();

    let result = patch_settings(&settings_path, &hook);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("corrupt settings.json"),
        "expected corrupt error, got: {err}"
    );
}

// --- write_hook_shim ---

#[test]
fn write_hook_shim_creates_executable_script() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks");
    let hook_script = hook_dir.join("pre-tool-use.sh");

    write_hook_shim(&hook_dir, &hook_script).unwrap();

    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(content.starts_with("#!/bin/sh\n"));
    assert!(
        content.contains("hook handle"),
        "expected 'hook handle' in script, got: {content}"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&hook_script).unwrap().permissions();
        assert!(perms.mode() & 0o111 != 0, "script should be executable");
    }
}

#[test]
fn write_hook_shim_quotes_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks");
    let hook_script = hook_dir.join("pre-tool-use.sh");

    write_hook_shim(&hook_dir, &hook_script).unwrap();

    let content = std::fs::read_to_string(&hook_script).unwrap();
    // The exec line should contain single quotes around the path
    assert!(
        content.contains("exec '"),
        "expected quoted path in script, got: {content}"
    );
}

// --- install_to (fix #8: test install with explicit paths) ---

#[test]
fn install_to_creates_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("global/tokf/hooks");
    let settings_path = dir.path().join("global/.claude/settings.json");

    install_to(&hook_dir, &settings_path).unwrap();

    let hook_script = hook_dir.join("pre-tool-use.sh");
    assert!(hook_script.exists(), "hook script should exist");
    assert!(settings_path.exists(), "settings.json should exist");

    let settings_content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&settings_content).unwrap();
    assert!(value["hooks"]["PreToolUse"].is_array());
}

#[test]
fn install_to_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    // Path must contain "tokf" and "hook" for idempotency detection
    let hook_dir = dir.path().join(".tokf/hooks");
    let settings_path = dir.path().join("settings.json");

    install_to(&hook_dir, &settings_path).unwrap();
    install_to(&hook_dir, &settings_path).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1, "should have one entry after double install");
}
