use serde::{Deserialize, Serialize};

/// Claude Code `PreToolUse` hook input (read from stdin).
#[derive(Debug, Clone, Deserialize)]
pub struct HookInput {
    pub tool_name: String,
    pub tool_input: ToolInput,
}

/// The `tool_input` payload from the hook.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolInput {
    pub command: Option<String>,
}

/// Response to send back when rewriting a command.
#[derive(Debug, Clone, Serialize)]
pub struct HookResponse {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: HookSpecificOutput,
}

/// The specific output that tells Claude Code to use a different command.
#[derive(Debug, Clone, Serialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    pub permission_decision: &'static str,
    #[serde(rename = "updatedInput")]
    pub updated_input: UpdatedInput,
}

/// The updated tool input with the rewritten command.
#[derive(Debug, Clone, Serialize)]
pub struct UpdatedInput {
    pub command: String,
}

impl HookResponse {
    /// Create a response that rewrites the command.
    pub const fn rewrite(command: String) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: "allow",
                updated_input: UpdatedInput { command },
            },
        }
    }
}

// --- Gemini CLI types ---

/// Gemini CLI `BeforeTool` hook input (read from stdin).
#[derive(Debug, Clone, Deserialize)]
pub struct GeminiHookInput {
    pub tool_name: String,
    pub tool_input: GeminiToolInput,
}

/// The `tool_input` payload from the Gemini hook.
#[derive(Debug, Clone, Deserialize)]
pub struct GeminiToolInput {
    pub command: Option<String>,
}

/// Response to send back to Gemini CLI when rewriting a command.
#[derive(Debug, Clone, Serialize)]
pub struct GeminiHookResponse {
    pub decision: &'static str,
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: GeminiHookSpecificOutput,
}

/// The Gemini-specific output containing the rewritten tool input.
#[derive(Debug, Clone, Serialize)]
pub struct GeminiHookSpecificOutput {
    pub tool_input: GeminiRewrittenInput,
}

/// The rewritten command for Gemini CLI.
#[derive(Debug, Clone, Serialize)]
pub struct GeminiRewrittenInput {
    pub command: String,
}

impl GeminiHookResponse {
    /// Create a Gemini response that rewrites the command.
    pub const fn rewrite(command: String) -> Self {
        Self {
            decision: "allow",
            hook_specific_output: GeminiHookSpecificOutput {
                tool_input: GeminiRewrittenInput { command },
            },
        }
    }
}

// --- Cursor types ---

/// Cursor `preToolUse` hook input (read from stdin).
#[derive(Debug, Clone, Deserialize)]
pub struct CursorHookInput {
    pub tool_name: String,
    pub tool_input: CursorToolInput,
}

/// The `tool_input` payload from the Cursor hook.
#[derive(Debug, Clone, Deserialize)]
pub struct CursorToolInput {
    pub command: Option<String>,
}

/// Response to send back to Cursor when rewriting a command.
#[derive(Debug, Clone, Serialize)]
pub struct CursorHookResponse {
    pub permission: &'static str,
    pub updated_input: CursorUpdatedInput,
}

/// The rewritten command for Cursor.
#[derive(Debug, Clone, Serialize)]
pub struct CursorUpdatedInput {
    pub command: String,
}

impl CursorHookResponse {
    /// Create a Cursor response that rewrites the command.
    pub const fn rewrite(command: String) -> Self {
        Self {
            permission: "allow",
            updated_input: CursorUpdatedInput { command },
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_bash_tool_input() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.tool_input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn deserialize_non_bash_tool() {
        let json = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/foo"}}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Read");
        assert!(input.tool_input.command.is_none());
    }

    #[test]
    fn deserialize_bash_no_command() {
        let json = r#"{"tool_name":"Bash","tool_input":{}}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert!(input.tool_input.command.is_none());
    }

    #[test]
    fn serialize_hook_response() {
        let response = HookResponse::rewrite("tokf run git status".to_string());
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(
            value["hookSpecificOutput"]["updatedInput"]["command"],
            "tokf run git status"
        );
    }

    #[test]
    fn response_round_trip() {
        let response = HookResponse::rewrite("tokf run cargo test".to_string());
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["hookSpecificOutput"]["updatedInput"]["command"],
            "tokf run cargo test"
        );
    }

    #[test]
    fn deserialize_extra_fields_ignored() {
        let json = r#"{"tool_name":"Bash","tool_input":{"command":"ls","timeout":5000},"session_id":"abc"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.tool_input.command.as_deref(), Some("ls"));
    }

    // --- Gemini CLI types ---

    #[test]
    fn deserialize_gemini_run_shell_command() {
        let json = r#"{"tool_name":"run_shell_command","tool_input":{"command":"git status"}}"#;
        let input: GeminiHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "run_shell_command");
        assert_eq!(input.tool_input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn serialize_gemini_hook_response() {
        let response = GeminiHookResponse::rewrite("tokf run git status".to_string());
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["decision"], "allow");
        assert_eq!(
            value["hookSpecificOutput"]["tool_input"]["command"],
            "tokf run git status"
        );
    }

    #[test]
    fn deserialize_gemini_no_command() {
        let json = r#"{"tool_name":"run_shell_command","tool_input":{}}"#;
        let input: GeminiHookInput = serde_json::from_str(json).unwrap();
        assert!(input.tool_input.command.is_none());
    }

    // --- Cursor types ---

    #[test]
    fn deserialize_cursor_shell_command() {
        let json = r#"{"tool_name":"Shell","tool_input":{"command":"cargo test"}}"#;
        let input: CursorHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "Shell");
        assert_eq!(input.tool_input.command.as_deref(), Some("cargo test"));
    }

    #[test]
    fn serialize_cursor_hook_response() {
        let response = CursorHookResponse::rewrite("tokf run cargo test".to_string());
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["permission"], "allow");
        assert_eq!(value["updated_input"]["command"], "tokf run cargo test");
    }

    #[test]
    fn deserialize_cursor_no_command() {
        let json = r#"{"tool_name":"Shell","tool_input":{}}"#;
        let input: CursorHookInput = serde_json::from_str(json).unwrap();
        assert!(input.tool_input.command.is_none());
    }
}
