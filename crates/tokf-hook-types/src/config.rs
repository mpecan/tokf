use serde::{Deserialize, Serialize};

use crate::engine::ExternalEngineConfig;

const fn default_true() -> bool {
    true
}

/// User-provided overrides loaded from `rewrites.toml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RewriteConfig {
    /// Additional skip patterns (commands matching these are never rewritten).
    pub skip: Option<SkipConfig>,

    /// Pipe stripping and prefer-less-context behaviour.
    pub pipe: Option<PipeConfig>,

    /// User-defined rewrite rules (checked before auto-generated filter rules).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrite: Vec<RewriteRule>,

    /// Permission engine configuration (external sub-hook delegation).
    pub permissions: Option<PermissionsConfig>,
}

/// Configuration for the permission decision engine.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PermissionsConfig {
    /// Which engine to use: `"builtin"` (default) or `"external"`.
    #[serde(default)]
    pub engine: PermissionEngineType,

    /// Configuration for the external engine (required when `engine = "external"`).
    pub external: Option<ExternalEngineConfig>,
}

/// Which permission engine to use.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionEngineType {
    /// Built-in deny/ask rule matching from Claude Code settings.json.
    #[default]
    Builtin,
    /// Delegate to an external process (sub-hook).
    External,
}

/// Controls how tokf handles piped commands during rewriting.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PipeConfig {
    /// Whether to strip simple pipes (tail/head/grep) when a filter matches.
    /// Default: true (current behaviour).
    #[serde(default = "default_true")]
    pub strip: bool,

    /// When true and a pipe is stripped, inject `--prefer-less` so that at
    /// runtime tokf compares filtered vs piped output and uses whichever is
    /// smaller.
    #[serde(default)]
    pub prefer_less: bool,
}

/// Extra skip patterns from user config.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SkipConfig {
    /// Regex patterns — if any matches the command, rewriting is skipped.
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// A single rewrite rule: match a command and replace it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewriteRule {
    /// Regex pattern to match against the command string.
    #[serde(rename = "match")]
    pub match_pattern: String,

    /// Replacement template. Supports `{0}` (full match), `{1}`, `{2}`, etc.
    pub replace: String,
}
