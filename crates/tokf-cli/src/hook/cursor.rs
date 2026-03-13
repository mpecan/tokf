use std::path::Path;

use crate::runner;

/// Install the Cursor `preToolUse` hook.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool, tokf_bin: &str, install_context: bool) -> anyhow::Result<()> {
    let (hook_dir, cursor_dir) = super::resolve_paths(global, ".cursor")?;
    let hooks_json_path = cursor_dir.join("hooks.json");
    let rules_dir = cursor_dir.join("rules");
    install_to(
        &hook_dir,
        &hooks_json_path,
        &rules_dir,
        tokf_bin,
        install_context,
    )
}

/// Core install logic with explicit paths (testable).
pub(crate) fn install_to(
    hook_dir: &Path,
    hooks_json_path: &Path,
    rules_dir: &Path,
    tokf_bin: &str,
    install_context: bool,
) -> anyhow::Result<()> {
    let hook_script = hook_dir.join("cursor-pre-tool-use.sh");
    super::write_hook_shim(hook_dir, &hook_script, tokf_bin, "--format cursor")?;
    patch_hooks_json(hooks_json_path, &hook_script)?;

    eprintln!("[tokf] Cursor hook installed");
    eprintln!("[tokf]   script: {}", hook_script.display());
    eprintln!("[tokf]   hooks: {}", hooks_json_path.display());

    if install_context {
        super::write_context_doc(rules_dir)?;
        eprintln!("[tokf]   context: {}", rules_dir.join("TOKF.md").display());
    }

    Ok(())
}

/// Patch Cursor hooks.json to register the `preToolUse` hook.
///
/// Cursor uses a different structure from Claude Code / Gemini — each hook entry
/// is a flat object with `matcher`, `type`, and `command` at the top level
/// (rather than nested in a `hooks` array), so we use a dedicated function.
fn patch_hooks_json(hooks_json_path: &Path, hook_script: &Path) -> anyhow::Result<()> {
    let mut config: serde_json::Value = if hooks_json_path.exists() {
        let content = std::fs::read_to_string(hooks_json_path)?;
        serde_json::from_str(&content).map_err(|e| {
            anyhow::anyhow!("corrupt hooks.json at {}: {e}", hooks_json_path.display())
        })?
    } else {
        serde_json::json!({ "version": 1 })
    };

    let hook_command = runner::shell_escape(
        hook_script
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("hook script path is not valid UTF-8"))?,
    );

    let tokf_hook_entry = serde_json::json!({
        "matcher": "Shell",
        "type": "command",
        "command": hook_command
    });

    let hooks = config
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.json is not an object"))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let pre_tool_use = hooks
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.json hooks is not an object"))?
        .entry("preToolUse")
        .or_insert_with(|| serde_json::json!([]));

    let arr = pre_tool_use
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks.preToolUse is not an array"))?;

    // Remove existing tokf entries (idempotent install)
    arr.retain(|entry| {
        let is_tokf = entry
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|cmd| cmd.contains("tokf") && cmd.contains("hook"));
        !is_tokf
    });

    arr.push(tokf_hook_entry);

    if let Some(parent) = hooks_json_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&config)?;
    let tmp_path = hooks_json_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, hooks_json_path)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_files() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let hooks_json = dir.path().join(".cursor/hooks.json");
        let rules_dir = dir.path().join(".cursor/rules");

        install_to(&hook_dir, &hooks_json, &rules_dir, "tokf", false).unwrap();

        assert!(hook_dir.join("cursor-pre-tool-use.sh").exists());
        assert!(hooks_json.exists());

        let content = std::fs::read_to_string(&hooks_json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["version"], 1);
        assert!(value["hooks"]["preToolUse"].is_array());
        assert_eq!(value["hooks"]["preToolUse"][0]["matcher"], "Shell");
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let hooks_json = dir.path().join(".cursor/hooks.json");
        let rules_dir = dir.path().join(".cursor/rules");

        install_to(&hook_dir, &hooks_json, &rules_dir, "tokf", false).unwrap();
        install_to(&hook_dir, &hooks_json, &rules_dir, "tokf", false).unwrap();

        let content = std::fs::read_to_string(&hooks_json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = value["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "should have one entry after double install");
    }

    #[test]
    fn hook_shim_has_cursor_format_flag() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join("hooks");
        let hook_script = hook_dir.join("cursor-pre-tool-use.sh");

        super::super::write_hook_shim(&hook_dir, &hook_script, "tokf", "--format cursor").unwrap();

        let content = std::fs::read_to_string(&hook_script).unwrap();
        assert!(
            content.contains("--format cursor"),
            "shim should use --format cursor, got: {content}"
        );
    }

    #[test]
    fn install_creates_context_doc() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let hooks_json = dir.path().join(".cursor/hooks.json");
        let rules_dir = dir.path().join(".cursor/rules");

        install_to(&hook_dir, &hooks_json, &rules_dir, "tokf", true).unwrap();

        assert!(rules_dir.join("TOKF.md").exists());
        let content = std::fs::read_to_string(rules_dir.join("TOKF.md")).unwrap();
        assert!(content.contains("🗜️"));
    }

    #[test]
    fn install_preserves_existing_hooks() {
        let dir = TempDir::new().unwrap();
        let hooks_json = dir.path().join("hooks.json");
        let hook = dir.path().join("hook.sh");

        std::fs::write(
            &hooks_json,
            r#"{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "matcher": "Shell",
        "type": "command",
        "command": "/other/tool.sh"
      }
    ]
  }
}"#,
        )
        .unwrap();

        patch_hooks_json(&hooks_json, &hook).unwrap();

        let content = std::fs::read_to_string(&hooks_json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = value["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "should have both hooks");
    }
}
