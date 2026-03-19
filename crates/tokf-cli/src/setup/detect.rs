/// Identifies which AI coding tool was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    GeminiCli,
    Cursor,
    Cline,
    Windsurf,
    Copilot,
    Aider,
    OpenCode,
    Codex,
}

impl Tool {
    /// The `--tool` value expected by `tokf hook install`.
    pub const fn cli_value(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::GeminiCli => "gemini-cli",
            Self::Cursor => "cursor",
            Self::Cline => "cline",
            Self::Windsurf => "windsurf",
            Self::Copilot => "copilot",
            Self::Aider => "aider",
            Self::OpenCode => "opencode",
            Self::Codex => "codex",
        }
    }
}

/// A tool that was detected on this machine.
#[derive(Debug)]
pub struct DetectedTool {
    pub tool: Tool,
    pub display_name: &'static str,
    /// What evidence was found (e.g. "binary in PATH", "~/.claude/ exists").
    pub evidence: String,
    /// Whether this tool supports the skill install step.
    pub supports_skill: bool,
}

/// Detect all supported AI tools installed on this machine.
pub fn detect_all() -> Vec<DetectedTool> {
    let home = dirs::home_dir();
    let mut found = Vec::new();
    detect_dir_tools(home.as_deref(), &mut found);
    detect_binary_tools(&mut found);
    found
}

fn push_if_detected(
    found: &mut Vec<DetectedTool>,
    tool: Tool,
    display_name: &'static str,
    supports_skill: bool,
    evidence: Option<String>,
) {
    if let Some(evidence) = evidence {
        found.push(DetectedTool {
            tool,
            display_name,
            evidence,
            supports_skill,
        });
    }
}

/// Tools detected via home directory presence (and optionally binary).
fn detect_dir_tools(home: Option<&std::path::Path>, found: &mut Vec<DetectedTool>) {
    push_if_detected(
        found,
        Tool::ClaudeCode,
        "Claude Code",
        true,
        detect_binary_or_dir(home, Some("claude"), Some(".claude")),
    );
    push_if_detected(
        found,
        Tool::GeminiCli,
        "Gemini CLI",
        false,
        detect_binary_or_dir(home, Some("gemini"), Some(".gemini")),
    );
    push_if_detected(
        found,
        Tool::Cursor,
        "Cursor",
        false,
        detect_binary_or_dir(home, None, Some(".cursor")),
    );
    push_if_detected(found, Tool::Cline, "Cline", false, detect_cline(home));
    push_if_detected(
        found,
        Tool::Windsurf,
        "Windsurf",
        false,
        detect_windsurf(home),
    );
    push_if_detected(
        found,
        Tool::OpenCode,
        "OpenCode",
        false,
        detect_binary_or_dir(home, Some("opencode"), Some(".opencode")),
    );
}

/// Tools detected via binary presence only.
fn detect_binary_tools(found: &mut Vec<DetectedTool>) {
    push_if_detected(
        found,
        Tool::Copilot,
        "GitHub Copilot",
        false,
        has_binary("gh").then(|| "`gh` binary in PATH".into()),
    );
    push_if_detected(
        found,
        Tool::Aider,
        "Aider",
        false,
        has_binary("aider").then(|| "binary in PATH".into()),
    );
    push_if_detected(
        found,
        Tool::Codex,
        "OpenAI Codex CLI",
        false,
        has_binary("codex").then(|| "binary in PATH".into()),
    );
}

fn has_binary(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Check for a binary in PATH and/or a directory under `$HOME`.
fn detect_binary_or_dir(
    home: Option<&std::path::Path>,
    binary: Option<&str>,
    dir: Option<&str>,
) -> Option<String> {
    if let Some(b) = binary
        && has_binary(b)
    {
        return Some("binary in PATH".into());
    }
    if let Some(d) = dir {
        let path = home?.join(d);
        if path.is_dir() {
            return Some(format!("~/{d}/ exists"));
        }
    }
    None
}

fn detect_cline(home: Option<&std::path::Path>) -> Option<String> {
    let vscode_ext = home?.join(".vscode").join("extensions");
    if !vscode_ext.is_dir() {
        return None;
    }
    let entries = std::fs::read_dir(&vscode_ext).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("saoudrizwan.claude-dev") || name.starts_with("sapio") {
            return Some(format!("extension found: {name}"));
        }
    }
    None
}

fn detect_windsurf(home: Option<&std::path::Path>) -> Option<String> {
    for dir in [".codeium/windsurf", ".windsurf"] {
        let path = home?.join(dir);
        if path.is_dir() {
            return Some(format!("~/{dir}/ exists"));
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn detect_all_returns_vec() {
        let tools = detect_all();
        for t in &tools {
            assert!(!t.display_name.is_empty());
            assert!(!t.evidence.is_empty());
        }
    }

    #[test]
    fn detect_binary_or_dir_returns_none_for_missing() {
        let result = detect_binary_or_dir(
            Some(std::path::Path::new("/nonexistent")),
            None,
            Some("foo"),
        );
        assert!(result.is_none());
    }

    #[test]
    fn detect_binary_or_dir_returns_some_for_existing_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        let result = detect_binary_or_dir(Some(dir.path()), None, Some("child"));
        assert!(result.is_some());
        assert!(result.unwrap().contains("exists"));
    }

    #[test]
    fn tool_cli_value_roundtrip() {
        assert_eq!(Tool::ClaudeCode.cli_value(), "claude-code");
        assert_eq!(Tool::GeminiCli.cli_value(), "gemini-cli");
        assert_eq!(Tool::Codex.cli_value(), "codex");
    }
}
