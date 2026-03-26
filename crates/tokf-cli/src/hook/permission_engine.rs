//! External permission engine — sub-hook delegation.
//!
//! When configured, tokf delegates the permission decision to an external
//! process (e.g. Dippy). The engine receives the original hook JSON on stdin
//! and returns a standard hook response on stdout. tokf extracts only the
//! permission decision from that response and applies it to its own
//! rewritten command.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::permissions::PermissionVerdict;
use super::types::HookFormat;

pub use tokf_hook_types::{ErrorFallback, ExternalEngineConfig};

/// Error from the external permission engine.
#[derive(Debug)]
pub enum EngineError {
    /// Could not spawn the engine process.
    SpawnFailed(std::io::Error),
    /// Engine process timed out.
    Timeout,
    /// Engine exited with non-zero status.
    NonZeroExit(Option<i32>),
    /// Stdout was not valid JSON.
    InvalidJson(String),
    /// Could not extract a permission decision from the response.
    NoVerdict,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "could not spawn engine: {e}"),
            Self::Timeout => write!(f, "engine timed out"),
            Self::NonZeroExit(Some(code)) => write!(f, "engine exited with code {code}"),
            Self::NonZeroExit(None) => write!(f, "engine terminated by signal"),
            Self::InvalidJson(s) => write!(f, "engine returned invalid JSON: {s}"),
            Self::NoVerdict => write!(f, "could not extract permission from engine response"),
        }
    }
}

/// Spawn the external engine, feed it the original hook JSON on stdin,
/// and extract the permission verdict from its hook response.
///
/// # Errors
///
/// Returns `EngineError` if the engine fails to spawn, times out, exits
/// with non-zero status, or returns unparseable output.
pub fn check_with_engine(
    config: &ExternalEngineConfig,
    hook_json: &str,
    format: HookFormat,
) -> Result<PermissionVerdict, EngineError> {
    let format_str = config.resolve_format(format);
    let resolved_args: Vec<String> = config
        .args
        .iter()
        .map(|a| a.replace("{format}", &format_str))
        .collect();

    let mut child = Command::new(&config.command)
        .args(&resolved_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(EngineError::SpawnFailed)?;

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(hook_json.as_bytes())
    {
        eprintln!("[tokf] warning: failed to write to engine stdin: {e}");
    }

    // Wait with timeout: spawn a thread to read stdout, use channel for timeout.
    let timeout = Duration::from_millis(config.timeout_ms);
    let stdout_pipe = child.stdout.take();
    let handle = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stdout_pipe {
            std::io::Read::read_to_end(&mut pipe, &mut buf)?;
        }
        Ok(buf)
    });

    // Poll child with deadline.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_bytes = handle.join().ok().and_then(Result::ok).unwrap_or_default();
                // Always try to parse stdout first — engines like rippy/dippy
                // use exit code 2 for ask/deny but still produce valid JSON.
                // Only fall back to NonZeroExit if parsing fails.
                match parse_engine_output(&stdout_bytes, format) {
                    Ok(verdict) => return Ok(verdict),
                    Err(_) if !status.success() => {
                        return Err(EngineError::NonZeroExit(status.code()));
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = handle.join();
                    return Err(EngineError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = handle.join();
                return Err(EngineError::SpawnFailed(e));
            }
        }
    }
}

/// Parse engine stdout into a permission verdict.
fn parse_engine_output(
    stdout_bytes: &[u8],
    format: HookFormat,
) -> Result<PermissionVerdict, EngineError> {
    let stdout = String::from_utf8_lossy(stdout_bytes);
    let json: Value = serde_json::from_str(stdout.trim())
        .map_err(|_| EngineError::InvalidJson(stdout.into_owned()))?;
    extract_verdict(&json, format).ok_or(EngineError::NoVerdict)
}

/// Extract the permission verdict from a hook response JSON based on format.
///
/// Returns `None` if the response doesn't contain a recognisable verdict.
pub fn extract_verdict(json: &Value, format: HookFormat) -> Option<PermissionVerdict> {
    let decision_str = match format {
        HookFormat::ClaudeCode => json
            .get("hookSpecificOutput")?
            .get("permissionDecision")
            .and_then(Value::as_str),
        HookFormat::Gemini => json.get("decision").and_then(Value::as_str),
        HookFormat::Cursor => json.get("permission").and_then(Value::as_str),
    };

    // For all formats: field present with "allow" → Allow,
    // field present with "deny" → Deny, field absent or "ask" → Ask.
    match decision_str {
        Some("allow") => Some(PermissionVerdict::Allow),
        Some("deny") => Some(PermissionVerdict::Deny),
        Some("ask") | None => Some(PermissionVerdict::Ask),
        Some(_) => None, // Unknown value — caller treats as error.
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // --- extract_verdict tests ---

    #[test]
    fn claude_code_allow() {
        let json = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": { "command": "git status" }
            }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::ClaudeCode),
            Some(PermissionVerdict::Allow)
        );
    }

    #[test]
    fn claude_code_deny() {
        let json = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "updatedInput": { "command": "rm -rf /" }
            }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::ClaudeCode),
            Some(PermissionVerdict::Deny)
        );
    }

    #[test]
    fn claude_code_ask_when_field_absent() {
        let json = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "updatedInput": { "command": "git push" }
            }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::ClaudeCode),
            Some(PermissionVerdict::Ask)
        );
    }

    #[test]
    fn claude_code_unknown_value() {
        let json = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "maybe",
                "updatedInput": { "command": "git status" }
            }
        });
        assert_eq!(extract_verdict(&json, HookFormat::ClaudeCode), None);
    }

    #[test]
    fn claude_code_missing_hook_specific_output() {
        let json = serde_json::json!({ "something": "else" });
        assert_eq!(extract_verdict(&json, HookFormat::ClaudeCode), None);
    }

    #[test]
    fn gemini_allow() {
        let json = serde_json::json!({
            "decision": "allow",
            "hookSpecificOutput": {
                "tool_input": { "command": "git status" }
            }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::Gemini),
            Some(PermissionVerdict::Allow)
        );
    }

    #[test]
    fn gemini_ask_when_field_absent() {
        let json = serde_json::json!({
            "hookSpecificOutput": {
                "tool_input": { "command": "git push" }
            }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::Gemini),
            Some(PermissionVerdict::Ask)
        );
    }

    #[test]
    fn cursor_allow() {
        let json = serde_json::json!({
            "permission": "allow",
            "updated_input": { "command": "cargo test" }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::Cursor),
            Some(PermissionVerdict::Allow)
        );
    }

    #[test]
    fn cursor_ask_when_field_absent() {
        let json = serde_json::json!({
            "updated_input": { "command": "git push" }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::Cursor),
            Some(PermissionVerdict::Ask)
        );
    }

    #[test]
    fn cursor_deny() {
        let json = serde_json::json!({
            "permission": "deny",
            "updated_input": { "command": "rm -rf /" }
        });
        assert_eq!(
            extract_verdict(&json, HookFormat::Cursor),
            Some(PermissionVerdict::Deny)
        );
    }

    // --- check_with_engine subprocess tests ---

    /// Write a mock engine script and return its path.
    #[cfg(unix)]
    fn write_mock_engine(dir: &std::path::Path, name: &str, script: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().to_string()
    }

    fn default_config(command: String) -> ExternalEngineConfig {
        ExternalEngineConfig {
            command,
            args: vec![],
            timeout_ms: 5000,
            on_error: ErrorFallback::Ask,
            format_map: std::collections::HashMap::new(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn engine_returns_allow() {
        let dir = tempfile::TempDir::new().unwrap();
        let response = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":"git status"}}}"#;
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!("#!/bin/sh\ncat >/dev/null\necho '{response}'"),
        );
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Allow);
    }

    #[cfg(unix)]
    #[test]
    fn engine_returns_ask_when_no_permission_field() {
        let dir = tempfile::TempDir::new().unwrap();
        let response = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{"command":"git push"}}}"#;
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!("#!/bin/sh\ncat >/dev/null\necho '{response}'"),
        );
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"git push"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Ask);
    }

    #[cfg(unix)]
    #[test]
    fn engine_non_zero_exit() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmd = write_mock_engine(dir.path(), "engine.sh", "#!/bin/sh\ncat >/dev/null\nexit 1");
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert!(matches!(result, Err(EngineError::NonZeroExit(Some(1)))));
    }

    #[cfg(unix)]
    #[test]
    fn engine_non_zero_exit_with_valid_json_uses_verdict() {
        // Engines like rippy use exit code 2 for ask/deny but still produce valid JSON.
        let dir = tempfile::TempDir::new().unwrap();
        let response = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","updatedInput":{"command":"rm -rf /"}}}"#;
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!("#!/bin/sh\ncat >/dev/null\necho '{response}'\nexit 2"),
        );
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"rm -rf /"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Ask);
    }

    #[cfg(unix)]
    #[test]
    fn engine_garbage_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            "#!/bin/sh\ncat >/dev/null\necho 'not json'",
        );
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert!(matches!(result, Err(EngineError::InvalidJson(_))));
    }

    #[cfg(unix)]
    #[test]
    fn engine_timeout() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            "#!/bin/sh\ncat >/dev/null\nsleep 10",
        );
        let mut config = default_config(cmd);
        config.timeout_ms = 100;
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert!(matches!(result, Err(EngineError::Timeout)));
    }

    #[test]
    fn engine_spawn_failure() {
        let config = default_config("/nonexistent/binary".to_string());
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert!(matches!(result, Err(EngineError::SpawnFailed(_))));
    }

    #[cfg(unix)]
    #[test]
    fn engine_receives_stdin() {
        // Verify the engine actually receives the hook JSON on stdin.
        let dir = tempfile::TempDir::new().unwrap();
        let marker_file = dir.path().join("received.json");
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!(
                "#!/bin/sh\ncat > {}\necho '{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{{\"command\":\"ls\"}}}}}}'",
                marker_file.display()
            ),
        );
        let config = default_config(cmd);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"git status"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Allow);
        let received = std::fs::read_to_string(&marker_file).unwrap();
        assert_eq!(received, hook_json);
    }

    #[cfg(unix)]
    #[test]
    fn engine_with_args() {
        let dir = tempfile::TempDir::new().unwrap();
        let marker_file = dir.path().join("args.txt");
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!(
                "#!/bin/sh\necho \"$@\" > {}\ncat >/dev/null\necho '{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{{\"command\":\"ls\"}}}}}}'",
                marker_file.display()
            ),
        );
        let mut config = default_config(cmd);
        config.args = vec!["hook".to_string(), "handle".to_string()];
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Allow);
        let args = std::fs::read_to_string(&marker_file).unwrap();
        assert_eq!(args.trim(), "hook handle");
    }

    #[cfg(unix)]
    #[test]
    fn engine_format_substitution() {
        let dir = tempfile::TempDir::new().unwrap();
        let marker_file = dir.path().join("args.txt");
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!(
                "#!/bin/sh\necho \"$@\" > {}\ncat >/dev/null\necho '{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{{\"command\":\"ls\"}}}}}}'",
                marker_file.display()
            ),
        );
        let mut config = default_config(cmd);
        config.args = vec!["--mode".to_string(), "{format}".to_string()];
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Allow);
        let args = std::fs::read_to_string(&marker_file).unwrap();
        assert_eq!(args.trim(), "--mode claude-code");
    }

    #[cfg(unix)]
    #[test]
    fn engine_format_map_override() {
        let dir = tempfile::TempDir::new().unwrap();
        let marker_file = dir.path().join("args.txt");
        let cmd = write_mock_engine(
            dir.path(),
            "engine.sh",
            &format!(
                "#!/bin/sh\necho \"$@\" > {}\ncat >/dev/null\necho '{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\",\"updatedInput\":{{\"command\":\"ls\"}}}}}}'",
                marker_file.display()
            ),
        );
        let mut config = default_config(cmd);
        config.args = vec!["--mode".to_string(), "{format}".to_string()];
        config.format_map =
            std::collections::HashMap::from([("claude-code".to_string(), "claude".to_string())]);
        let hook_json = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let result = check_with_engine(&config, hook_json, HookFormat::ClaudeCode);
        assert_eq!(result.unwrap(), PermissionVerdict::Allow);
        let args = std::fs::read_to_string(&marker_file).unwrap();
        assert_eq!(args.trim(), "--mode claude");
    }

    #[test]
    fn resolve_format_default() {
        let config = default_config("test".to_string());
        assert_eq!(config.resolve_format(HookFormat::ClaudeCode), "claude-code");
        assert_eq!(config.resolve_format(HookFormat::Gemini), "gemini");
        assert_eq!(config.resolve_format(HookFormat::Cursor), "cursor");
    }

    #[test]
    fn resolve_format_with_map() {
        let mut config = default_config("test".to_string());
        config.format_map = std::collections::HashMap::from([
            ("claude-code".to_string(), "anthropic".to_string()),
            ("gemini".to_string(), "google".to_string()),
        ]);
        assert_eq!(config.resolve_format(HookFormat::ClaudeCode), "anthropic");
        assert_eq!(config.resolve_format(HookFormat::Gemini), "google");
        assert_eq!(config.resolve_format(HookFormat::Cursor), "cursor");
    }
}
