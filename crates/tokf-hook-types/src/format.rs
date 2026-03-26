/// Which hook format is being processed — determines response shape
/// and which JSON field carries the permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookFormat {
    /// Claude Code: `hookSpecificOutput.permissionDecision`
    ClaudeCode,
    /// Gemini CLI: `decision`
    Gemini,
    /// Cursor: `permission`
    Cursor,
}

impl HookFormat {
    /// Default string identifier for this format.
    ///
    /// Used as the default `{format}` template value in external engine args.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Gemini => "gemini",
            Self::Cursor => "cursor",
        }
    }
}
