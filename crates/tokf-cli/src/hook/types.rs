use serde::{Deserialize, Serialize};

pub use tokf_hook_types::HookFormat;

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

/// The specific output that tells Claude Code the permission decision.
#[derive(Debug, Clone, Serialize)]
pub struct HookSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: &'static str,
    #[serde(rename = "permissionDecision", skip_serializing_if = "Option::is_none")]
    pub permission_decision: Option<&'static str>,
    #[serde(
        rename = "permissionDecisionReason",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(rename = "updatedInput")]
    pub updated_input: UpdatedInput,
}

/// The updated tool input with the rewritten command.
#[derive(Debug, Clone, Serialize)]
pub struct UpdatedInput {
    pub command: String,
}

impl HookResponse {
    /// Create a response that rewrites and auto-allows the command.
    pub const fn rewrite(command: String, reason: Option<String>) -> Self {
        Self::with_decision("allow", command, reason)
    }

    /// Create a response that rewrites the command and explicitly asks
    /// Claude Code to prompt the user.
    pub const fn rewrite_ask(command: String, reason: Option<String>) -> Self {
        Self::with_decision("ask", command, reason)
    }

    /// Create a deny response with an optional reason.
    pub const fn deny(command: String, reason: Option<String>) -> Self {
        Self::with_decision("deny", command, reason)
    }

    const fn with_decision(
        decision: &'static str,
        command: String,
        reason: Option<String>,
    ) -> Self {
        Self {
            hook_specific_output: HookSpecificOutput {
                hook_event_name: "PreToolUse",
                permission_decision: Some(decision),
                permission_decision_reason: reason,
                updated_input: UpdatedInput { command },
            },
        }
    }
}

// --- Gemini CLI types ---
// Gemini input types share the same JSON shape as Claude Code (HookInput/ToolInput),
// so no separate Gemini input type is needed. Cursor uses a different format (CursorInput).

/// Response to send back to Gemini CLI when rewriting a command.
#[derive(Debug, Clone, Serialize)]
pub struct GeminiHookResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    /// Create a Gemini response that rewrites and auto-allows the command.
    pub const fn rewrite(command: String, reason: Option<String>) -> Self {
        Self::with_decision("allow", command, reason)
    }

    /// Create a Gemini response that rewrites but defers permission to the tool.
    /// Gemini has no "ask" concept — maps to "deny".
    pub const fn rewrite_ask(command: String, reason: Option<String>) -> Self {
        Self::with_decision("deny", command, reason)
    }

    /// Create a Gemini deny response with an optional reason.
    /// Gemini has no "ask" concept — both ask and deny map to "deny".
    pub const fn deny(command: String, reason: Option<String>) -> Self {
        Self::with_decision("deny", command, reason)
    }

    const fn with_decision(
        decision: &'static str,
        command: String,
        reason: Option<String>,
    ) -> Self {
        Self {
            decision: Some(decision),
            reason,
            hook_specific_output: GeminiHookSpecificOutput {
                tool_input: GeminiRewrittenInput { command },
            },
        }
    }
}

// --- Cursor types ---
// Cursor's `beforeShellExecution` hook sends `command` at the top level,
// unlike Claude Code / Gemini which nest it under `tool_input`.

/// Cursor `beforeShellExecution` hook input (read from stdin).
#[derive(Debug, Clone, Deserialize)]
pub struct CursorInput {
    pub command: Option<String>,
}

/// Response to send back to Cursor when rewriting a command.
#[derive(Debug, Clone, Serialize)]
pub struct CursorHookResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<&'static str>,
    #[serde(rename = "userMessage", skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(rename = "agentMessage", skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
    pub updated_input: CursorUpdatedInput,
}

/// The rewritten command for Cursor.
#[derive(Debug, Clone, Serialize)]
pub struct CursorUpdatedInput {
    pub command: String,
}

impl CursorHookResponse {
    /// Create a Cursor response that rewrites and auto-allows the command.
    pub fn rewrite(command: String, reason: Option<String>) -> Self {
        Self::with_permission("allow", command, reason)
    }

    /// Create a Cursor response that rewrites and explicitly asks the user.
    pub fn rewrite_ask(command: String, reason: Option<String>) -> Self {
        Self::with_permission("ask", command, reason)
    }

    /// Create a Cursor deny response with an optional reason.
    pub fn deny(command: String, reason: Option<String>) -> Self {
        Self::with_permission("deny", command, reason)
    }

    fn with_permission(permission: &'static str, command: String, reason: Option<String>) -> Self {
        Self {
            permission: Some(permission),
            user_message: reason.clone(),
            agent_message: reason,
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
    fn serialize_hook_response_allow() {
        let response = HookResponse::rewrite("tokf run git status".to_string(), None);
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "allow");
        assert!(
            value["hookSpecificOutput"]
                .get("permissionDecisionReason")
                .is_none(),
            "reason should be absent when None"
        );
        assert_eq!(
            value["hookSpecificOutput"]["updatedInput"]["command"],
            "tokf run git status"
        );
    }

    #[test]
    fn serialize_hook_response_allow_with_reason() {
        let response = HookResponse::rewrite(
            "tokf run git status".to_string(),
            Some("safe command".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecisionReason"],
            "safe command"
        );
    }

    #[test]
    fn serialize_hook_response_ask_includes_decision() {
        let response = HookResponse::rewrite_ask(
            "tokf run git push".to_string(),
            Some("requires confirmation".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "ask");
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecisionReason"],
            "requires confirmation"
        );
        assert_eq!(
            value["hookSpecificOutput"]["updatedInput"]["command"],
            "tokf run git push"
        );
    }

    #[test]
    fn serialize_hook_response_deny() {
        let response = HookResponse::deny(
            "rm -rf /".to_string(),
            Some("dangerous command".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            value["hookSpecificOutput"]["permissionDecisionReason"],
            "dangerous command"
        );
    }

    #[test]
    fn response_round_trip() {
        let response = HookResponse::rewrite("tokf run cargo test".to_string(), None);
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
    // Gemini inputs share the same JSON shape as HookInput,
    // so deserialization is tested via HookInput above. Only response
    // serialization differs per protocol.

    #[test]
    fn deserialize_gemini_run_shell_command() {
        let json = r#"{"tool_name":"run_shell_command","tool_input":{"command":"git status"}}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.tool_name, "run_shell_command");
        assert_eq!(input.tool_input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn serialize_gemini_hook_response_allow() {
        let response = GeminiHookResponse::rewrite("tokf run git status".to_string(), None);
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["decision"], "allow");
        assert!(
            value.get("reason").is_none(),
            "reason should be absent when None"
        );
        assert_eq!(
            value["hookSpecificOutput"]["tool_input"]["command"],
            "tokf run git status"
        );
    }

    #[test]
    fn serialize_gemini_hook_response_ask_maps_to_deny() {
        let response = GeminiHookResponse::rewrite_ask(
            "tokf run git push".to_string(),
            Some("needs approval".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value["decision"], "deny",
            "Gemini has no ask — maps to deny"
        );
        assert_eq!(value["reason"], "needs approval");
    }

    #[test]
    fn serialize_gemini_hook_response_deny() {
        let response =
            GeminiHookResponse::deny("rm -rf /".to_string(), Some("dangerous".to_string()));
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["decision"], "deny");
        assert_eq!(value["reason"], "dangerous");
    }

    // --- Cursor types ---

    #[test]
    fn deserialize_cursor_before_shell_execution() {
        let json = r#"{"conversation_id":"abc","command":"git status","cwd":"/tmp","hook_event_name":"beforeShellExecution"}"#;
        let input: CursorInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.command.as_deref(), Some("git status"));
    }

    #[test]
    fn deserialize_cursor_no_command() {
        let json = r#"{"conversation_id":"abc","cwd":"/tmp"}"#;
        let input: CursorInput = serde_json::from_str(json).unwrap();
        assert!(input.command.is_none());
    }

    #[test]
    fn serialize_cursor_hook_response_allow() {
        let response = CursorHookResponse::rewrite("tokf run cargo test".to_string(), None);
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["permission"], "allow");
        assert!(
            value.get("userMessage").is_none(),
            "reason should be absent when None"
        );
        assert!(
            value.get("agentMessage").is_none(),
            "reason should be absent when None"
        );
        assert_eq!(value["updated_input"]["command"], "tokf run cargo test");
    }

    #[test]
    fn serialize_cursor_hook_response_ask_includes_permission() {
        let response = CursorHookResponse::rewrite_ask(
            "tokf run git push".to_string(),
            Some("needs confirmation".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["permission"], "ask");
        assert_eq!(value["userMessage"], "needs confirmation");
        assert_eq!(value["agentMessage"], "needs confirmation");
        assert_eq!(value["updated_input"]["command"], "tokf run git push");
    }

    #[test]
    fn serialize_cursor_hook_response_deny() {
        let response = CursorHookResponse::deny(
            "rm -rf /".to_string(),
            Some("dangerous command".to_string()),
        );
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["permission"], "deny");
        assert_eq!(value["userMessage"], "dangerous command");
        assert_eq!(value["agentMessage"], "dangerous command");
    }
}
