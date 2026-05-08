use std::path::{Path, PathBuf};

use anyhow::Context;

use super::{
    patch_json_hook_config_with_command, patch_md_with_reference, resolve_paths, write_context_doc,
    write_hook_shim_with_global_args,
};
use crate::runner;

const SKILL_MD: &str = include_str!("../../skills/codex-run/SKILL.md");
const DISCOVER_SKILL_MD: &str = include_str!("../../skills/codex-discover/SKILL.md");

struct CodexSkill {
    dir_name: &'static str,
    content: &'static str,
}

const CODEX_SKILLS: &[CodexSkill] = &[
    CodexSkill {
        dir_name: "tokf-run",
        content: SKILL_MD,
    },
    CodexSkill {
        dir_name: "tokf-discover",
        content: DISCOVER_SKILL_MD,
    },
];

/// Install Codex CLI hook and skills (tokf-run + tokf-discover).
///
/// # Errors
///
/// Returns an error if the hook, hook config, or skill files cannot be written.
pub fn install(global: bool, tokf_bin: &str, install_context: bool) -> anyhow::Result<()> {
    let (hook_dir, codex_dir) = resolve_paths(global, ".codex")?;
    install_hook_to(&hook_dir, &codex_dir, tokf_bin, install_context)?;

    let parent = if global {
        let home = dirs::home_dir().context("could not determine home directory")?;
        home.join(".agents/skills")
    } else {
        PathBuf::from(".agents/skills")
    };
    for skill in CODEX_SKILLS {
        write_skill_file(&parent.join(skill.dir_name), skill.content)?;
    }
    eprintln!("[tokf] Codex skills installed to {}", parent.display());
    eprintln!("[tokf] Codex will auto-discover the skills on next start.");
    Ok(())
}

fn install_hook_to(
    hook_dir: &Path,
    codex_dir: &Path,
    tokf_bin: &str,
    install_context: bool,
) -> anyhow::Result<()> {
    let hooks_json = codex_dir.join("hooks.json");
    let hook_script = write_codex_hook_shim(hook_dir, tokf_bin)?;
    let hook_command = codex_hook_command(&hook_script)?;
    patch_json_hook_config_with_command(&hooks_json, &hook_command, "PreToolUse", "Bash", None)?;

    eprintln!("[tokf] Codex hook installed");
    eprintln!("[tokf]   script: {}", hook_script.display());
    eprintln!("[tokf]   hooks: {}", hooks_json.display());

    if install_context {
        let created = write_context_doc(codex_dir)?;
        patch_md_with_reference(codex_dir, "AGENTS.md")?;
        if created {
            eprintln!("[tokf]   context: {}", codex_dir.join("TOKF.md").display());
        } else {
            eprintln!(
                "[tokf]   context: {} (already exists, skipped)",
                codex_dir.join("TOKF.md").display()
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookScriptPlatform {
    Unix,
    Windows,
}

const fn current_hook_script_platform() -> HookScriptPlatform {
    if cfg!(windows) {
        HookScriptPlatform::Windows
    } else {
        HookScriptPlatform::Unix
    }
}

fn write_codex_hook_shim(hook_dir: &Path, tokf_bin: &str) -> anyhow::Result<PathBuf> {
    write_codex_hook_shim_for_platform(hook_dir, tokf_bin, current_hook_script_platform())
}

fn write_codex_hook_shim_for_platform(
    hook_dir: &Path,
    tokf_bin: &str,
    platform: HookScriptPlatform,
) -> anyhow::Result<PathBuf> {
    let hook_script = hook_dir.join(codex_hook_script_name(platform));
    match platform {
        HookScriptPlatform::Unix => write_hook_shim_with_global_args(
            hook_dir,
            &hook_script,
            tokf_bin,
            "--no-cache",
            "--format codex",
        )?,
        HookScriptPlatform::Windows => {
            std::fs::create_dir_all(hook_dir)?;
            let escaped_bin = if tokf_bin == "tokf" {
                tokf_bin.to_string()
            } else {
                cmd_quote(tokf_bin)
            };
            let content = format!(
                "@echo off\r\n{escaped_bin} --no-cache hook handle --format codex\r\nexit /b %ERRORLEVEL%\r\n"
            );
            std::fs::write(&hook_script, content)?;
        }
    }
    Ok(hook_script)
}

const fn codex_hook_script_name(platform: HookScriptPlatform) -> &'static str {
    match platform {
        HookScriptPlatform::Unix => "codex-pre-tool-use.sh",
        HookScriptPlatform::Windows => "codex-pre-tool-use.cmd",
    }
}

fn codex_hook_command(hook_script: &Path) -> anyhow::Result<String> {
    codex_hook_command_for_platform(hook_script, current_hook_script_platform())
}

fn codex_hook_command_for_platform(
    hook_script: &Path,
    platform: HookScriptPlatform,
) -> anyhow::Result<String> {
    let script = hook_script
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("hook script path is not valid UTF-8"))?;
    Ok(match platform {
        HookScriptPlatform::Unix => runner::shell_escape(script),
        HookScriptPlatform::Windows => cmd_quote(script),
    })
}

fn cmd_quote(arg: &str) -> String {
    format!("\"{}\"", arg.replace('"', "\"\""))
}

#[cfg(test)]
fn install_to(skill_dir: &Path) -> anyhow::Result<()> {
    write_skill_file(skill_dir, SKILL_MD)?;
    eprintln!(
        "[tokf] Codex skill installed to {}",
        skill_dir.join("SKILL.md").display()
    );
    eprintln!("[tokf] Codex will auto-discover the skill on next start.");
    Ok(())
}

fn write_skill_file(skill_dir: &Path, content: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(skill_dir)
        .with_context(|| format!("failed to create skill dir: {}", skill_dir.display()))?;

    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(&skill_file, content)
        .with_context(|| format!("failed to write skill file: {}", skill_file.display()))?;
    eprintln!("[tokf] wrote {}", skill_file.display());

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_skill_file() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();

        assert!(skill_dir.join("SKILL.md").exists());
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();
        install_to(&skill_dir).unwrap();

        // Only one file exists, no errors
        let entries: Vec<_> = std::fs::read_dir(&skill_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn skill_file_has_frontmatter() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(
            content.starts_with("---\n"),
            "SKILL.md should start with YAML frontmatter"
        );
        assert!(
            content.contains("name: tokf-run"),
            "SKILL.md frontmatter should include name: tokf-run"
        );
    }

    #[test]
    fn skill_file_has_description() {
        let content = SKILL_MD;
        assert!(
            content.contains("description:"),
            "SKILL.md frontmatter should include description"
        );
    }

    #[test]
    fn skill_file_mentions_tokf_run() {
        let content = SKILL_MD;
        assert!(
            content.contains("tokf run"),
            "SKILL.md should instruct using tokf run"
        );
    }

    #[test]
    fn skill_file_has_no_double_prefix_rule() {
        let content = SKILL_MD;
        assert!(
            content.contains("double-prefix"),
            "SKILL.md should warn against double-prefixing"
        );
    }

    #[test]
    fn skill_file_has_fail_safe_rule() {
        let content = SKILL_MD;
        assert!(
            content.contains("Fail-safe"),
            "SKILL.md should include fail-safe instruction"
        );
    }

    #[test]
    fn embedded_content_is_not_empty() {
        assert!(!SKILL_MD.is_empty(), "SKILL_MD should not be empty");
        assert!(
            !DISCOVER_SKILL_MD.is_empty(),
            "DISCOVER_SKILL_MD should not be empty"
        );
    }

    #[test]
    fn discover_skill_has_frontmatter() {
        assert!(
            DISCOVER_SKILL_MD.starts_with("---\n"),
            "discover SKILL.md should start with YAML frontmatter"
        );
        assert!(
            DISCOVER_SKILL_MD.contains("name: tokf-discover"),
            "discover SKILL.md should include name: tokf-discover"
        );
    }

    #[test]
    fn install_hook_to_creates_codex_hook_config() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let codex_dir = dir.path().join(".codex");

        install_hook_to(&hook_dir, &codex_dir, "tokf", false).unwrap();

        let hook_script = hook_dir.join(codex_hook_script_name(current_hook_script_platform()));
        assert!(hook_script.exists());
        let script = std::fs::read_to_string(&hook_script).unwrap();
        assert!(script.contains("--no-cache hook handle --format codex"));

        let hooks_json = codex_dir.join("hooks.json");
        let content = std::fs::read_to_string(hooks_json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    }

    #[test]
    fn unix_codex_shim_uses_no_cache_shell_script() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");

        let hook_script =
            write_codex_hook_shim_for_platform(&hook_dir, "tokf", HookScriptPlatform::Unix)
                .unwrap();

        assert_eq!(hook_script.file_name().unwrap(), "codex-pre-tool-use.sh");
        let script = std::fs::read_to_string(&hook_script).unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("exec tokf --no-cache hook handle --format codex"));

        let command =
            codex_hook_command_for_platform(&hook_script, HookScriptPlatform::Unix).unwrap();
        assert!(command.starts_with('\''));
        assert!(command.ends_with('\''));
    }

    #[test]
    fn windows_codex_shim_uses_cmd_script_and_cmd_quoting() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join("tokf hooks");

        let hook_script = write_codex_hook_shim_for_platform(
            &hook_dir,
            r"C:\Program Files\tokf\tokf.exe",
            HookScriptPlatform::Windows,
        )
        .unwrap();

        assert_eq!(hook_script.file_name().unwrap(), "codex-pre-tool-use.cmd");
        let script = std::fs::read_to_string(&hook_script).unwrap();
        assert!(script.starts_with("@echo off\r\n"));
        assert!(
            script.contains(
                r#""C:\Program Files\tokf\tokf.exe" --no-cache hook handle --format codex"#
            )
        );
        assert!(script.ends_with("exit /b %ERRORLEVEL%\r\n"));

        let command =
            codex_hook_command_for_platform(&hook_script, HookScriptPlatform::Windows).unwrap();
        assert!(command.starts_with('"'));
        assert!(command.ends_with('"'));
        assert!(command.contains("tokf hooks"));
    }

    #[test]
    fn install_hook_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join(".tokf/hooks");
        let codex_dir = dir.path().join(".codex");

        install_hook_to(&hook_dir, &codex_dir, "tokf", false).unwrap();
        install_hook_to(&hook_dir, &codex_dir, "tokf", false).unwrap();

        let hooks_json = codex_dir.join("hooks.json");
        let content = std::fs::read_to_string(hooks_json).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }
}
