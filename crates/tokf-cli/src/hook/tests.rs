use super::*;

// --- handle_json ---

#[test]
fn handle_bash_with_no_matching_filter() {
    // No filters in search path, so no rewrite should happen
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-cmd"}}"#;
    assert_eq!(handle_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_non_bash_tool_passes_through() {
    let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/foo"}}"#;
    assert_eq!(handle_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_bash_no_command_passes_through() {
    let json = r#"{"tool_name":"Bash","tool_input":{}}"#;
    assert_eq!(handle_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_invalid_json_passes_through() {
    assert_eq!(handle_json("not json"), HookOutcome::PassThrough);
}

#[test]
fn handle_empty_input_passes_through() {
    assert_eq!(handle_json(""), HookOutcome::PassThrough);
}

#[test]
fn handle_tokf_command_not_rewritten() {
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"tokf run git status"}}"#;
    assert_eq!(handle_json(json), HookOutcome::PassThrough);
}

// --- handle_json_with_rules (fix #9: test the rewrite path) ---

#[test]
fn handle_json_with_rules_rewrites_matching_command() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite to occur for matching command"
    );
}

#[test]
fn handle_json_with_rules_no_match_returns_false() {
    let dir = tempfile::TempDir::new().unwrap();
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"unknown-xyz-cmd-99"}}"#;
    let config = RewriteConfig::default();
    let result = handle_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(result, HookOutcome::PassThrough);
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
    let result = handle_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite for env-prefixed matching command"
    );
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
    let result = handle_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite for multiple env vars prefix"
    );
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
    let result = handle_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite for env var + strippable pipe"
    );
}

#[test]
fn handle_json_env_prefixed_tokf_command_not_rewritten() {
    // DEBUG=1 tokf run ... must not be rewritten — same as tokf run ... passthrough.
    let json = r#"{"tool_name":"Bash","tool_input":{"command":"DEBUG=1 tokf run git status"}}"#;
    assert_eq!(handle_json(json), HookOutcome::PassThrough);
}

// --- patch_json_hook_config ---

#[test]
fn patch_creates_new_settings_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let settings = dir.path().join(".claude/settings.json");
    let hook = dir.path().join("hook.sh");

    patch_json_hook_config(&settings, &hook, "PreToolUse", "Bash", None).unwrap();

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

    patch_json_hook_config(&settings_path, &hook, "PreToolUse", "Bash", None).unwrap();

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
    patch_json_hook_config(&settings_path, &hook, "PreToolUse", "Bash", None).unwrap();
    patch_json_hook_config(&settings_path, &hook, "PreToolUse", "Bash", None).unwrap();

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

    patch_json_hook_config(&settings_path, &hook, "PreToolUse", "Bash", None).unwrap();

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

    patch_json_hook_config(&settings_path, hook, "PreToolUse", "Bash", None).unwrap();

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

    let result = patch_json_hook_config(&settings_path, &hook, "PreToolUse", "Bash", None);
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

    write_hook_shim(&hook_dir, &hook_script, "tokf", "").unwrap();

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
fn write_hook_shim_uses_bare_tokf() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks");
    let hook_script = hook_dir.join("pre-tool-use.sh");

    write_hook_shim(&hook_dir, &hook_script, "tokf", "").unwrap();

    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(
        content.contains("exec tokf hook handle"),
        "expected bare tokf in script, got: {content}"
    );
}

#[test]
fn write_hook_shim_custom_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks");
    let hook_script = hook_dir.join("pre-tool-use.sh");

    write_hook_shim(&hook_dir, &hook_script, "/opt/bin/tokf", "").unwrap();

    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(
        content.contains("exec '/opt/bin/tokf' hook handle"),
        "expected shell-escaped custom path, got: {content}"
    );
}

#[test]
fn write_hook_shim_path_with_spaces() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks");
    let hook_script = hook_dir.join("pre-tool-use.sh");

    write_hook_shim(&hook_dir, &hook_script, "/home/my user/bin/tokf", "").unwrap();

    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(
        content.contains("exec '/home/my user/bin/tokf' hook handle"),
        "path with spaces should be shell-escaped, got: {content}"
    );
}

// --- install_to (fix #8: test install with explicit paths) ---

#[test]
fn install_to_creates_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join("global/tokf/hooks");
    let settings_path = dir.path().join("global/.claude/settings.json");

    install_to(&hook_dir, &settings_path, "tokf", false).unwrap();

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

    install_to(&hook_dir, &settings_path, "tokf", false).unwrap();
    install_to(&hook_dir, &settings_path, "tokf", false).unwrap();

    let content = std::fs::read_to_string(&settings_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    let arr = value["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(arr.len(), 1, "should have one entry after double install");
}

#[test]
fn install_creates_tokf_md() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join(".tokf/hooks");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    install_to(&hook_dir, &settings_path, "tokf", true).unwrap();

    let tokf_md = claude_dir.join("TOKF.md");
    assert!(tokf_md.exists(), "TOKF.md should exist");
    let content = std::fs::read_to_string(&tokf_md).unwrap();
    assert!(
        content.contains("🗜️"),
        "TOKF.md should contain compression indicator, got: {content}"
    );
    assert!(
        content.contains("tokf raw last"),
        "TOKF.md should mention tokf raw last, got: {content}"
    );
}

#[test]
fn install_patches_claude_md_with_reference() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join(".tokf/hooks");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    // Pre-existing CLAUDE.md content
    std::fs::write(claude_dir.join("CLAUDE.md"), "# My Project\n").unwrap();

    install_to(&hook_dir, &settings_path, "tokf", true).unwrap();

    let content = std::fs::read_to_string(claude_dir.join("CLAUDE.md")).unwrap();
    assert!(
        content.contains("@TOKF.md"),
        "CLAUDE.md should contain @TOKF.md reference, got: {content}"
    );
    assert!(
        content.contains("# My Project"),
        "existing content should be preserved, got: {content}"
    );
}

#[test]
fn install_idempotent_claude_md() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join(".tokf/hooks");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    install_to(&hook_dir, &settings_path, "tokf", true).unwrap();
    install_to(&hook_dir, &settings_path, "tokf", true).unwrap();

    let content = std::fs::read_to_string(claude_dir.join("CLAUDE.md")).unwrap();
    let count = content.matches("@TOKF.md").count();
    assert_eq!(
        count, 1,
        "should have exactly one @TOKF.md reference after double install, got: {count}"
    );
}

#[test]
fn install_preserves_existing_tokf_md() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join(".tokf/hooks");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    let custom = "custom user content\n";
    std::fs::write(claude_dir.join("TOKF.md"), custom).unwrap();

    install_to(&hook_dir, &settings_path, "tokf", true).unwrap();

    let content = std::fs::read_to_string(claude_dir.join("TOKF.md")).unwrap();
    assert_eq!(
        content, custom,
        "existing TOKF.md should not be overwritten, got: {content}"
    );
}

#[test]
fn install_no_context_skips_tokf_md() {
    let dir = tempfile::TempDir::new().unwrap();
    let hook_dir = dir.path().join(".tokf/hooks");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let settings_path = claude_dir.join("settings.json");

    install_to(&hook_dir, &settings_path, "tokf", false).unwrap();

    let tokf_md = claude_dir.join("TOKF.md");
    assert!(
        !tokf_md.exists(),
        "TOKF.md should not exist when install_context is false"
    );
}

// --- handle_gemini_json_with_config ---

#[test]
fn handle_gemini_rewrites_matching_command() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let json = r#"{"tool_name":"run_shell_command","tool_input":{"command":"git status"}}"#;
    let config = RewriteConfig::default();
    let result = handle_gemini_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite for Gemini matching command"
    );
}

#[test]
fn handle_gemini_non_shell_passes_through() {
    let json = r#"{"tool_name":"read_file","tool_input":{"path":"/tmp/foo"}}"#;
    assert_eq!(handle_gemini_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_gemini_no_command_passes_through() {
    let json = r#"{"tool_name":"run_shell_command","tool_input":{}}"#;
    assert_eq!(handle_gemini_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_gemini_invalid_json_passes_through() {
    assert_eq!(handle_gemini_json("not json"), HookOutcome::PassThrough);
}

// --- handle_cursor_json_with_config ---

#[test]
fn handle_cursor_rewrites_matching_command() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    // Cursor's beforeShellExecution sends command at the top level
    let json = r#"{"command":"cargo test","cwd":"/tmp","hook_event_name":"beforeShellExecution"}"#;
    let config = RewriteConfig::default();
    let result = handle_cursor_json_with_rules(json, &config, &[dir.path().to_path_buf()]);
    assert_eq!(
        result,
        HookOutcome::Allow,
        "expected rewrite for Cursor matching command"
    );
}

#[test]
fn handle_cursor_no_command_passes_through() {
    let json = r#"{"cwd":"/tmp","hook_event_name":"beforeShellExecution"}"#;
    assert_eq!(handle_cursor_json(json), HookOutcome::PassThrough);
}

#[test]
fn handle_cursor_invalid_json_passes_through() {
    assert_eq!(handle_cursor_json("not json"), HookOutcome::PassThrough);
}

// --- append_or_replace_section ---

#[test]
fn append_or_replace_section_missing_end_marker_appends_instead() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.md");
    // File has start marker but no end marker — should NOT truncate
    std::fs::write(
        &path,
        "before\n<!-- tokf:start -->\nold content\nuser data\n",
    )
    .unwrap();

    append_or_replace_section(&path, || {
        "<!-- tokf:start -->\nupdated\n<!-- tokf:end -->".to_string()
    })
    .unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("user data"),
        "should not truncate content after orphaned start marker, got: {content}"
    );
    assert!(
        content.contains("updated"),
        "should append new section, got: {content}"
    );
}

#[test]
fn append_or_replace_section_creates_new_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.md");

    append_or_replace_section(&path, || {
        "<!-- tokf:start -->\ntokf content\n<!-- tokf:end -->".to_string()
    })
    .unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("<!-- tokf:start -->"));
    assert!(content.contains("tokf content"));
    assert!(content.contains("<!-- tokf:end -->"));
}

#[test]
fn append_or_replace_section_appends_to_existing() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.md");
    std::fs::write(&path, "# Existing\n").unwrap();

    append_or_replace_section(&path, || {
        "<!-- tokf:start -->\nnew section\n<!-- tokf:end -->".to_string()
    })
    .unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("# Existing\n"));
    assert!(content.contains("new section"));
}

#[test]
fn append_or_replace_section_replaces_existing() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.md");
    std::fs::write(
        &path,
        "before\n<!-- tokf:start -->\nold\n<!-- tokf:end -->\nafter\n",
    )
    .unwrap();

    append_or_replace_section(&path, || {
        "<!-- tokf:start -->\nupdated\n<!-- tokf:end -->".to_string()
    })
    .unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("before\n"));
    assert!(content.contains("updated"));
    assert!(!content.contains("old"));
    assert!(content.contains("after"));
}

#[test]
fn append_or_replace_section_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("test.md");

    let section_fn = || "<!-- tokf:start -->\ncontent\n<!-- tokf:end -->".to_string();

    append_or_replace_section(&path, section_fn).unwrap();
    append_or_replace_section(&path, section_fn).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let count = content.matches("<!-- tokf:start -->").count();
    assert_eq!(count, 1, "should have exactly one tokf section");
}
