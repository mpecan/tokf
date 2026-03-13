use std::path::Path;

/// Install the Gemini CLI `BeforeTool` hook.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool, tokf_bin: &str, install_context: bool) -> anyhow::Result<()> {
    let (hook_dir, gemini_dir) = super::resolve_paths(global, ".gemini")?;
    let settings_path = gemini_dir.join("settings.json");
    install_to(
        &hook_dir,
        &settings_path,
        &gemini_dir,
        tokf_bin,
        install_context,
    )
}

/// Core install logic with explicit paths (testable).
pub(crate) fn install_to(
    hook_dir: &Path,
    settings_path: &Path,
    context_dir: &Path,
    tokf_bin: &str,
    install_context: bool,
) -> anyhow::Result<()> {
    let hook_script = hook_dir.join("gemini-before-tool.sh");
    super::write_hook_shim(hook_dir, &hook_script, tokf_bin, " --format gemini")?;
    super::patch_json_hook_config(
        settings_path,
        &hook_script,
        "BeforeTool",
        "run_shell_command",
        None,
    )?;

    eprintln!("[tokf] Gemini CLI hook installed");
    eprintln!("[tokf]   script: {}", hook_script.display());
    eprintln!("[tokf]   settings: {}", settings_path.display());

    if install_context {
        super::write_context_doc(context_dir)?;
        super::patch_md_with_reference(context_dir, "GEMINI.md")?;
        eprintln!(
            "[tokf]   context: {}",
            context_dir.join("TOKF.md").display()
        );
    }

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
        let settings_path = dir.path().join(".gemini/settings.json");
        let context_dir = dir.path().join(".gemini");

        install_to(&hook_dir, &settings_path, &context_dir, "tokf", false).unwrap();

        assert!(hook_dir.join("gemini-before-tool.sh").exists());
        assert!(settings_path.exists());

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(value["hooks"]["BeforeTool"].is_array());
        assert_eq!(
            value["hooks"]["BeforeTool"][0]["matcher"],
            "run_shell_command"
        );
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let settings_path = dir.path().join(".gemini/settings.json");
        let context_dir = dir.path().join(".gemini");

        install_to(&hook_dir, &settings_path, &context_dir, "tokf", false).unwrap();
        install_to(&hook_dir, &settings_path, &context_dir, "tokf", false).unwrap();

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = value["hooks"]["BeforeTool"].as_array().unwrap();
        assert_eq!(arr.len(), 1, "should have one entry after double install");
    }

    #[test]
    fn hook_shim_has_gemini_format_flag() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join("hooks");
        let hook_script = hook_dir.join("gemini-before-tool.sh");

        super::super::write_hook_shim(&hook_dir, &hook_script, "tokf", " --format gemini").unwrap();

        let content = std::fs::read_to_string(&hook_script).unwrap();
        assert!(
            content.contains("--format gemini"),
            "shim should use --format gemini, got: {content}"
        );
    }

    #[test]
    fn install_creates_context_docs() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let settings_path = dir.path().join(".gemini/settings.json");
        let context_dir = dir.path().join(".gemini");

        install_to(&hook_dir, &settings_path, &context_dir, "tokf", true).unwrap();

        assert!(context_dir.join("TOKF.md").exists());
        assert!(context_dir.join("GEMINI.md").exists());
        let gemini_md = std::fs::read_to_string(context_dir.join("GEMINI.md")).unwrap();
        assert!(gemini_md.contains("@TOKF.md"));
    }

    #[test]
    fn install_preserves_existing_hooks() {
        let dir = TempDir::new().unwrap();
        let settings_path = dir.path().join(".gemini/settings.json");
        let hook = dir.path().join("hook.sh");

        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command",
        "hooks": [{ "type": "command", "command": "/other/tool.sh" }]
      }
    ]
  }
}"#,
        )
        .unwrap();

        super::super::patch_json_hook_config(
            &settings_path,
            &hook,
            "BeforeTool",
            "run_shell_command",
            None,
        )
        .unwrap();

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        let arr = value["hooks"]["BeforeTool"].as_array().unwrap();
        assert_eq!(arr.len(), 2, "should have both hooks");
    }
}
