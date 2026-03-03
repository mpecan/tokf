#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
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

// --- tokf hook install --tool opencode ---

// R3: Verify OpenCode plugin file is created at the expected path.
#[test]
fn hook_install_opencode_creates_plugin_file() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--tool", "opencode"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install --tool opencode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plugin_file = dir.path().join(".opencode/plugins/tokf.ts");
    assert!(
        plugin_file.exists(),
        ".opencode/plugins/tokf.ts should exist after install"
    );
}

// R3: Running install twice produces exactly one file and no errors.
#[test]
fn hook_install_opencode_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();

    for _ in 0..2 {
        let output = tokf()
            .args(["hook", "install", "--tool", "opencode"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "hook install --tool opencode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let plugin_dir = dir.path().join(".opencode/plugins");
    let entries: Vec<_> = std::fs::read_dir(&plugin_dir).unwrap().collect();
    assert_eq!(
        entries.len(),
        1,
        "should have exactly one plugin file after double install"
    );
}

// R3: The generated plugin file must not contain the raw template placeholder.
#[test]
fn hook_install_opencode_embeds_tokf_path() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--tool", "opencode"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install --tool opencode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plugin_file = dir.path().join(".opencode/plugins/tokf.ts");
    let content = std::fs::read_to_string(&plugin_file).unwrap();
    assert!(
        !content.contains("{{TOKF_BIN}}"),
        "plugin file must not contain raw placeholder, got: {content}"
    );
}

// --- tokf hook install --path ---

#[test]
fn hook_install_custom_path_embeds_in_shim() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--path", "/custom/bin/tokf"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install --path failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let hook_script = dir.path().join(".tokf/hooks/pre-tool-use.sh");
    let content = std::fs::read_to_string(&hook_script).unwrap();
    assert!(
        content.contains("'/custom/bin/tokf'"),
        "expected custom path in hook shim, got: {content}"
    );
}

#[test]
fn hook_install_custom_path_opencode() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args([
            "hook",
            "install",
            "--tool",
            "opencode",
            "--path",
            "/custom/bin/tokf",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install --tool opencode --path failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plugin_file = dir.path().join(".opencode/plugins/tokf.ts");
    let content = std::fs::read_to_string(&plugin_file).unwrap();
    assert!(
        content.contains(r#"const TOKF_BIN = "/custom/bin/tokf";"#),
        "expected custom path in plugin, got: {content}"
    );
}

// --- tokf hook install --tool codex ---

#[test]
fn hook_install_codex_creates_skill_file() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--tool", "codex"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook install --tool codex failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let skill_file = dir.path().join(".agents/skills/tokf-run/SKILL.md");
    assert!(
        skill_file.exists(),
        ".agents/skills/tokf-run/SKILL.md should exist after install"
    );
}

#[test]
fn hook_install_codex_is_idempotent() {
    let dir = tempfile::TempDir::new().unwrap();

    for _ in 0..2 {
        let output = tokf()
            .args(["hook", "install", "--tool", "codex"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "hook install --tool codex failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let skill_dir = dir.path().join(".agents/skills/tokf-run");
    let entries: Vec<_> = std::fs::read_dir(&skill_dir).unwrap().collect();
    assert_eq!(
        entries.len(),
        1,
        "should have exactly one skill file after double install"
    );
}

#[test]
fn hook_install_codex_has_frontmatter() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--tool", "codex"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let skill_file = dir.path().join(".agents/skills/tokf-run/SKILL.md");
    let content = std::fs::read_to_string(&skill_file).unwrap();
    assert!(
        content.starts_with("---\n"),
        "SKILL.md should start with YAML frontmatter"
    );
    assert!(
        content.contains("name: tokf-run"),
        "SKILL.md should contain name: tokf-run"
    );
}

#[test]
fn hook_install_codex_shows_info_on_stderr() {
    let dir = tempfile::TempDir::new().unwrap();

    let output = tokf()
        .args(["hook", "install", "--tool", "codex"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Codex skill installed"),
        "expected install confirmation, got: {stderr}"
    );
    assert!(
        stderr.contains("SKILL.md"),
        "expected skill file path in output, got: {stderr}"
    );
}
