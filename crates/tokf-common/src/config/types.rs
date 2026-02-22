#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A command pattern — either a single string or a list of alternatives.
///
/// ```toml
/// command = "git push"                    # Single
/// command = ["pnpm test", "npm test"]     # Multiple: any variant
/// command = "npm run *"                   # Wildcard: * matches one word
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CommandPattern {
    Single(String),
    Multiple(Vec<String>),
}

impl CommandPattern {
    /// All pattern strings for this command.
    pub fn patterns(&self) -> &[String] {
        match self {
            Self::Single(s) => std::slice::from_ref(s),
            Self::Multiple(v) => v,
        }
    }

    /// Canonical (first) pattern string, used for display and dedup.
    pub fn first(&self) -> &str {
        match self {
            Self::Single(s) => s.as_str(),
            Self::Multiple(v) => v.first().map_or("", String::as_str),
        }
    }
}

impl Default for CommandPattern {
    fn default() -> Self {
        Self::Single(String::new())
    }
}

/// Top-level filter configuration, deserialized from a `.toml` file.
// FilterConfig has many independent boolean flags that map directly to TOML keys.
// Grouping them into enums would not improve clarity here.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterConfig {
    /// The command this filter applies to (e.g. "git push").
    pub command: CommandPattern,

    /// Optional override command to run instead of the matched command prefix.
    ///
    /// Use `{args}` to interpolate the user-supplied arguments that appear
    /// **after** the matched pattern words.  Example:
    /// ```toml
    /// run = "git log --oneline --no-decorate -n 20 {args}"
    /// ```
    ///
    /// # Transparent global flags and `run`
    ///
    /// When the runtime matches a command like `git -C /repo log`, the global
    /// flags (`-C /repo`) are skipped during pattern matching but are **not**
    /// included in `{args}`.  `{args}` only contains arguments that appear
    /// *after* the fully-matched pattern (`log` in this case), so the override
    /// command receives `{args} = []` and the `-C /repo` flags are silently
    /// dropped.
    ///
    /// If your override command must honour such flags, include them explicitly
    /// in the `run` value or omit `run` to let tokf reconstruct the command
    /// from `command_args[..words_consumed]` (which *does* preserve the flags).
    pub run: Option<String>,

    /// Patterns for lines to skip (applied before section parsing).
    #[serde(default)]
    pub skip: Vec<String>,

    /// Patterns for lines to keep (inverse of skip).
    #[serde(default)]
    pub keep: Vec<String>,

    /// Pipeline steps to run before filtering.
    #[serde(default)]
    pub step: Vec<Step>,

    /// Extract a single value from the output.
    pub extract: Option<ExtractRule>,

    /// Whole-output matchers checked before any line processing.
    #[serde(default)]
    pub match_output: Vec<MatchOutputRule>,

    /// State-machine sections for collecting lines into named groups.
    #[serde(default)]
    pub section: Vec<Section>,

    /// Branch taken when the command exits 0.
    pub on_success: Option<OutputBranch>,

    /// Branch taken when the command exits non-zero.
    pub on_failure: Option<OutputBranch>,

    /// Structured parsing rules (branch line, file grouping).
    pub parse: Option<ParseConfig>,

    /// Output formatting configuration.
    pub output: Option<OutputConfig>,

    /// Fallback behavior when no other rule matches.
    pub fallback: Option<FallbackConfig>,

    /// Per-line regex replacement steps, applied before skip/keep.
    #[serde(default)]
    pub replace: Vec<ReplaceRule>,

    /// Collapse consecutive identical lines (or within a sliding window).
    #[serde(default)]
    pub dedup: bool,

    /// Window size for dedup (default: consecutive only).
    pub dedup_window: Option<usize>,

    /// Strip ANSI escape sequences before skip/keep pattern matching.
    #[serde(default)]
    pub strip_ansi: bool,

    /// Trim leading/trailing whitespace from each line before skip/keep matching.
    #[serde(default)]
    pub trim_lines: bool,

    /// Remove all blank lines from the final output.
    #[serde(default)]
    pub strip_empty_lines: bool,

    /// Collapse consecutive blank lines into one in the final output.
    #[serde(default)]
    pub collapse_empty_lines: bool,

    /// Optional Lua/Luau script escape hatch.
    #[serde(default)]
    pub lua_script: Option<ScriptConfig>,

    /// Variant entries for context-aware filter delegation.
    #[serde(default)]
    pub variant: Vec<Variant>,
}

/// A pipeline step that runs a sub-command and captures its output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Command to run.
    pub run: String,

    /// Name to bind the output to in the template context.
    #[serde(rename = "as")]
    pub as_name: Option<String>,

    /// Whether this step is part of a pipeline. Reserved for Phase 2+; unused by
    /// current filter configs.
    pub pipeline: Option<bool>,
}

/// Extracts a value from text using a regex pattern and formats it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractRule {
    /// Regex pattern with capture groups.
    pub pattern: String,

    /// Output template using `{1}`, `{2}`, etc. for captures.
    pub output: String,
}

/// Matches against the full output and short-circuits with a fixed message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchOutputRule {
    /// Substring to search for in the combined output.
    pub contains: String,

    /// Output to emit if the substring is found.
    pub output: String,
}

/// A state-machine section that collects lines between enter/exit markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// Name of this section (for diagnostics/debugging).
    pub name: Option<String>,

    /// Regex that activates this section.
    pub enter: Option<String>,

    /// Regex that deactivates this section.
    pub exit: Option<String>,

    /// Regex that individual lines must match to be collected.
    #[serde(rename = "match")]
    pub match_pattern: Option<String>,

    /// Regex to split collected content into blocks.
    pub split_on: Option<String>,

    /// Variable name for the collected lines/blocks.
    pub collect_as: Option<String>,
}

/// Output branch for success/failure exit codes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputBranch {
    /// Template string for the output.
    pub output: Option<String>,

    /// Aggregation rule for collected sections.
    pub aggregate: Option<AggregateRule>,

    /// Number of lines to keep from the tail.
    pub tail: Option<usize>,

    /// Number of lines to keep from the head.
    pub head: Option<usize>,

    /// Patterns for lines to skip within this branch.
    #[serde(default)]
    pub skip: Vec<String>,

    /// Extract rule applied within this branch.
    pub extract: Option<ExtractRule>,
}

/// Aggregates values from a collected section using regex extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateRule {
    /// Name of the collected section to aggregate from.
    pub from: String,

    /// Regex pattern to extract numeric values.
    pub pattern: String,

    /// Name for the summed value.
    pub sum: Option<String>,

    /// Name for the count of matching entries.
    pub count_as: Option<String>,
}

/// Structured parsing configuration for status-like outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseConfig {
    /// Rule for extracting the branch name from the first line.
    pub branch: Option<LineExtract>,

    /// Rule for grouping file entries by status code.
    pub group: Option<GroupConfig>,
}

/// Extracts a value from a specific line number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineExtract {
    /// 1-based line number to extract from.
    pub line: usize,

    /// Regex pattern with capture groups.
    pub pattern: String,

    /// Output template using `{1}`, `{2}`, etc. for captures.
    pub output: String,
}

/// Groups lines by a key pattern and maps keys to human labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupConfig {
    /// Rule for extracting the group key from each line.
    pub key: ExtractRule,

    /// Map from raw key to human-readable label.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

/// Output formatting configuration for the final rendered result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Top-level output format template.
    pub format: Option<String>,

    /// Format template for each group count line.
    pub group_counts_format: Option<String>,

    /// Message to emit when there are no items to report.
    pub empty: Option<String>,
}

/// Fallback behavior when no specific rule matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Number of lines to keep from the tail as a last resort.
    pub tail: Option<usize>,
}

/// One per-line regex replacement step.
///
/// Pattern is applied to each line; on match, the line is replaced with the
/// interpolated output template. Capture groups use `{1}`, `{2}`, … syntax.
/// Multiple rules run in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceRule {
    pub pattern: String,
    pub output: String,
}

/// Supported scripting languages for the `[lua_script]` escape hatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptLang {
    Luau,
}

/// Lua/Luau script escape hatch configuration.
/// Exactly one of `file` or `source` must be set.
/// `file` paths resolve relative to the current working directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptConfig {
    pub lang: ScriptLang,
    /// Path to a `.luau` file (resolved relative to CWD).
    pub file: Option<String>,
    /// Inline Luau source.
    pub source: Option<String>,
}

/// Detection criteria for a filter variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantDetect {
    /// File paths to check in CWD (pre-execution detection).
    #[serde(default)]
    pub files: Vec<String>,
    /// Regex pattern to match against command output (post-execution fallback).
    pub output_pattern: Option<String>,
}

/// A variant entry that delegates to a specialized child filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    /// Human-readable name for this variant.
    pub name: String,
    /// Detection criteria (file-based and/or output-pattern).
    pub detect: VariantDetect,
    /// Filter name to delegate to (relative path without `.toml`).
    pub filter: String,
}
