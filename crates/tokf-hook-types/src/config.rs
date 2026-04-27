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

    /// Debug/diagnostic settings (all off by default).
    pub debug: Option<DebugConfig>,

    /// Commands whose argv is opaque to tokf because it executes in a
    /// different shell environment (typically a remote host or container).
    /// User regex `[[rewrite]]` rules are not applied to these commands —
    /// only argv-preserving wraps (`tokf run <cmd>`) and pipe-flag injection
    /// remain.
    pub transparent: Option<TransparentConfig>,
}

/// "Transparent-arg" commands: their last argument is opaque shell code.
///
/// Built-in list (always active): `ssh`, `mosh`, `slogin`. The `commands`
/// field extends — does not replace — the built-in list. tokf must not
/// splice text into the argv of these commands via regex rewrites.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TransparentConfig {
    /// Additional command basenames to treat as transparent. Matched against
    /// the basename of the command's first word, so `kubectl` matches both
    /// `kubectl` and `/usr/local/bin/kubectl`.
    #[serde(default)]
    pub commands: Vec<String>,
}

/// Debug and diagnostic settings for the rewrite system.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DebugConfig {
    /// When true, log to stderr when the bash parser (rable) fails to parse a
    /// command. This helps diagnose "unmatched quote" errors by showing whether
    /// tokf fell back to simple string matching because the AST parse failed.
    #[serde(default)]
    pub log_parse_failures: bool,
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
