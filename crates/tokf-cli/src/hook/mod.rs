pub mod opencode;
pub mod types;

use std::io::Read;
use std::path::{Path, PathBuf};

use types::{HookInput, HookResponse};

use crate::rewrite;
use crate::rewrite::types::RewriteConfig;
use crate::runner;

/// Process a `PreToolUse` hook invocation.
///
/// Reads JSON from stdin, checks if it's a Bash tool call, rewrites the command
/// if a matching rule is found, and prints the response JSON to stdout.
///
/// Returns `Ok(true)` if a rewrite was emitted, `Ok(false)` for pass-through.
/// Errors are intentionally swallowed to never block commands.
pub fn handle() -> bool {
    handle_from_reader(&mut std::io::stdin())
}

/// Testable version that reads from any `Read` source.
pub(crate) fn handle_from_reader<R: Read>(reader: &mut R) -> bool {
    let mut input = String::new();
    if reader.read_to_string(&mut input).is_err() {
        return false;
    }

    handle_json(&input)
}

/// Core handle logic operating on a JSON string.
pub(crate) fn handle_json(json: &str) -> bool {
    let user_config = rewrite::load_user_config().unwrap_or_default();
    let search_dirs = crate::config::default_search_dirs();
    handle_json_with_config(json, &user_config, &search_dirs)
}

/// Fully injectable handle logic for testing.
pub(crate) fn handle_json_with_config(
    json: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
) -> bool {
    let Ok(hook_input) = serde_json::from_str::<HookInput>(json) else {
        return false;
    };

    // Only rewrite Bash tool calls
    if hook_input.tool_name != "Bash" {
        return false;
    }

    let Some(command) = hook_input.tool_input.command else {
        return false;
    };

    let rewritten = rewrite::rewrite_with_config(&command, user_config, search_dirs, false);

    if rewritten == command {
        return false;
    }

    let response = HookResponse::rewrite(rewritten);
    if let Ok(json) = serde_json::to_string(&response) {
        println!("{json}");
        return true;
    }

    false
}

/// Install the hook shim and register it in Claude Code settings.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    let (hook_dir, settings_path) = if global {
        let config = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?;
        let hook_dir = config.join("tokf/hooks");
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        let settings_path = home.join(".claude/settings.json");
        (hook_dir, settings_path)
    } else {
        let cwd = std::env::current_dir()?;
        let hook_dir = cwd.join(".tokf/hooks");
        let settings_path = cwd.join(".claude/settings.json");
        (hook_dir, settings_path)
    };

    install_to(&hook_dir, &settings_path)
}

/// Core install logic with explicit paths (testable).
pub(crate) fn install_to(hook_dir: &Path, settings_path: &Path) -> anyhow::Result<()> {
    let hook_script = hook_dir.join("pre-tool-use.sh");
    write_hook_shim(hook_dir, &hook_script)?;
    patch_settings(settings_path, &hook_script)?;

    eprintln!("[tokf] hook installed");
    eprintln!("[tokf]   script: {}", hook_script.display());
    eprintln!("[tokf]   settings: {}", settings_path.display());

    Ok(())
}

/// Write the hook shim script.
fn write_hook_shim(hook_dir: &Path, hook_script: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(hook_dir)?;

    let tokf_path = std::env::current_exe()?;
    let quoted = runner::shell_escape(&tokf_path.to_string_lossy());
    let content = format!("#!/bin/sh\nexec {quoted} hook handle\n");
    std::fs::write(hook_script, &content)?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(hook_script, perms)?;
    }

    Ok(())
}

/// Patch Claude Code settings.json to register the hook.
fn patch_settings(settings_path: &Path, hook_script: &Path) -> anyhow::Result<()> {
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)?;
        serde_json::from_str(&content).map_err(|e| {
            anyhow::anyhow!("corrupt settings.json at {}: {e}", settings_path.display())
        })?
    } else {
        serde_json::json!({})
    };

    let hook_command = runner::shell_escape(
        hook_script
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("hook script path is not valid UTF-8"))?,
    );

    let tokf_hook_entry = serde_json::json!({
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": hook_command }]
    });

    // Get or create hooks.PreToolUse array
    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json is not an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let pre_tool_use = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("settings.json hooks is not an object"))?
        .entry("PreToolUse")
        .or_insert_with(|| serde_json::json!([]));

    let arr = pre_tool_use
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.PreToolUse is not an array"))?;

    // Remove any existing tokf hook entries (idempotent install)
    arr.retain(|entry| {
        let dominated_by_tokf =
            entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|h| {
                        h.get("command")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|cmd| cmd.contains("tokf") && cmd.contains("hook"))
                    })
                });
        !dominated_by_tokf
    });

    arr.push(tokf_hook_entry);

    // Write atomically: write to temp file then rename
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&settings)?;
    let tmp_path = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, settings_path)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
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
}
