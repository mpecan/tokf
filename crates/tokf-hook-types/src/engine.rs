use serde::{Deserialize, Serialize};

/// Configuration for an external permission engine.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExternalEngineConfig {
    /// Path to the external engine binary (resolved via PATH if not absolute).
    pub command: String,

    /// Arguments passed to the engine. Use `{format}` as a placeholder for the
    /// tool format (e.g. `["hook", "handle", "--mode", "{format}"]`).
    /// The placeholder is replaced with the resolved format string before spawning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Timeout in milliseconds. Default: 5000 (5 seconds).
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// What to do when the engine fails (crash, timeout, bad output).
    #[serde(default)]
    pub on_error: ErrorFallback,

    /// Override the default format strings used for `{format}` substitution.
    /// Keys are the default names (`claude-code`, `gemini`, `cursor`);
    /// values are the replacements the engine expects.
    ///
    /// Example: `{ "claude-code" = "claude", "gemini" = "google" }`
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub format_map: std::collections::HashMap<String, String>,
}

/// Behaviour when the external engine fails.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ErrorFallback {
    /// Fail closed — prompt user for permission (default).
    #[default]
    Ask,
    /// Fail open — auto-allow.
    Allow,
    /// Fall back to built-in rule matching.
    Builtin,
}

impl Default for ExternalEngineConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            timeout_ms: default_timeout(),
            on_error: ErrorFallback::default(),
            format_map: std::collections::HashMap::new(),
        }
    }
}

impl ExternalEngineConfig {
    /// Resolve the format string for a given hook format,
    /// applying `format_map` overrides if present.
    pub fn resolve_format(&self, format: crate::HookFormat) -> String {
        let default = format.as_str();
        self.format_map
            .get(default)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
}

pub const fn default_timeout() -> u64 {
    5000
}
