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
                updated_input: UpdatedInput { command },
            },
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
        assert_eq!(
            json,
            r#"{"hookSpecificOutput":{"updatedInput":{"command":"tokf run git status"}}}"#
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
}
