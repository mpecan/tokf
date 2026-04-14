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
    #[serde(default, alias = "strip_lines_matching")]
    pub skip: Vec<String>,

    /// Patterns for lines to keep (inverse of skip).
    #[serde(default, alias = "keep_lines_matching")]
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

    /// Directory-tree restructuring for path-list outputs. See
    /// [`crate::config::tree::TreeConfig`] for the schema and
    /// `crates/tokf-filter/src/filter/tree.rs` for the algorithm.
    pub tree: Option<crate::config::tree::TreeConfig>,

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

    /// Chunk processing: split output into repeating structural blocks.
    #[serde(default)]
    pub chunk: Vec<ChunkConfig>,

    /// JSON extraction: parse stdout as JSON and extract values via `JSONPath`.
    pub json: Option<JsonConfig>,

    /// Variant entries for context-aware filter delegation.
    #[serde(default)]
    pub variant: Vec<Variant>,

    /// When true, append a hint line after the filtered output telling the reader
    /// how to retrieve the full, unfiltered output from history.
    ///
    /// Example hint appended to output:
    /// ```text
    /// [tokf] output filtered — to see what was omitted: `tokf history show --raw 42`
    /// ```
    ///
    /// This is useful for LLM consumers that need to know complete output is
    /// available even though tokf has compressed it.
    #[serde(default)]
    pub show_history_hint: bool,

    /// When true, prepend a directory of shim scripts to `PATH` before spawning
    /// the command. Each shim redirects through `tokf -c`, so commands invoked
    /// by sub-processes (e.g. git hooks) are automatically filtered.
    #[serde(default)]
    pub inject_path: bool,

    /// Argument prefixes that trigger passthrough mode (skip filter entirely).
    ///
    /// When any element in the user's remaining args starts with any prefix in
    /// this list, tokf runs the original command as-is without applying the
    /// `run` override or filter pipeline.
    #[serde(default)]
    pub passthrough_args: Vec<String>,

    /// Human-readable description of what this filter does.
    /// Used in `tokf ls`, search results, and publishing metadata.
    pub description: Option<String>,

    /// Maximum character width for output lines. Lines longer than this are
    /// truncated with a trailing `…` (within the budget). Applied as a final
    /// post-processing step after all other pipeline stages.
    pub truncate_lines_at: Option<usize>,

    /// Message to display when the filter produces empty output (all lines
    /// stripped). Without this, empty output is returned as-is.
    pub on_empty: Option<String>,

    /// Number of lines to keep from the head of the output, applied regardless
    /// of exit code. Branch-level `head` overrides this when present.
    #[serde(alias = "head_lines")]
    pub head: Option<usize>,

    /// Number of lines to keep from the tail of the output, applied regardless
    /// of exit code. Branch-level `tail` overrides this when present.
    #[serde(alias = "tail_lines")]
    pub tail: Option<usize>,

    /// Absolute maximum line count for the final output. Applied after all
    /// other line-limiting stages (head, tail, skip, etc.). Lines beyond
    /// this limit are silently dropped from the end.
    pub max_lines: Option<usize>,
}

impl FilterConfig {
    /// Returns `true` if any user arg matches a prefix in `passthrough_args`.
    pub fn should_passthrough(&self, remaining_args: &[String]) -> bool {
        if self.passthrough_args.is_empty() || remaining_args.is_empty() {
            return false;
        }
        remaining_args.iter().any(|arg| {
            self.passthrough_args
                .iter()
                .any(|prefix| !prefix.is_empty() && arg.starts_with(prefix.as_str()))
        })
    }
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
///
/// At least one of `contains` (literal substring) or `pattern` (regex) must be
/// set. When both are present, `contains` is tried first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchOutputRule {
    /// Substring to search for in the combined output.
    #[serde(default)]
    pub contains: Option<String>,

    /// Regex pattern to match against the combined output (RTK-compatible).
    /// Used when `contains` is not set, or as a secondary matcher.
    pub pattern: Option<String>,

    /// Output to emit if the match succeeds.
    #[serde(alias = "message")]
    pub output: String,

    /// Regex pattern — if this also matches the output, skip this rule.
    /// Prevents short-circuit rules from swallowing errors (RTK-compatible).
    pub unless: Option<String>,
}

impl MatchOutputRule {
    /// Validate that at least one of `contains` or `pattern` is set.
    ///
    /// # Errors
    ///
    /// Returns an error if both `contains` and `pattern` are `None`.
    pub fn validate(&self) -> Result<(), String> {
        if self.contains.is_none() && self.pattern.is_none() {
            return Err(
                "match_output rule must have at least one of `contains` or `pattern`".to_string(),
            );
        }
        Ok(())
    }
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

    /// Aggregation rule for collected sections (singular shorthand).
    pub aggregate: Option<AggregateRule>,

    /// Multiple aggregation rules for collected sections.
    #[serde(default)]
    pub aggregates: Vec<AggregateRule>,

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

/// Configuration for splitting output into repeating structural blocks.
///
/// Chunks split output at delimiter lines, extract structured data from
/// each block, and collect the results as a structured collection for
/// template rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkConfig {
    /// Regex that marks the start of each chunk.
    pub split_on: String,

    /// Whether the splitting line is included in the chunk (default: true).
    #[serde(default = "default_true")]
    pub include_split_line: bool,

    /// Variable name for the structured collection in templates.
    pub collect_as: String,

    /// Extract a named field from the split (header) line.
    pub extract: Option<ChunkExtract>,

    /// Per-chunk body line extractions (first match per rule wins).
    #[serde(default)]
    pub body_extract: Vec<ChunkBodyExtract>,

    /// Per-chunk aggregate rules (run within each chunk's lines).
    #[serde(default)]
    pub aggregate: Vec<ChunkAggregateRule>,

    /// Field name to group chunks by (merging numeric fields).
    pub group_by: Option<String>,

    /// When set alongside `group_by`, preserve each group's original items
    /// as a nested collection under this name instead of discarding them.
    pub children_as: Option<String>,
}

const fn default_true() -> bool {
    true
}

/// Per-chunk aggregation rule. Unlike branch-level `AggregateRule`, this does
/// not need a `from` field because it always operates on the chunk's own lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkAggregateRule {
    /// Regex pattern to extract numeric values.
    pub pattern: String,

    /// Name for the summed value.
    pub sum: Option<String>,

    /// Name for the count of matching entries.
    pub count_as: Option<String>,
}

/// Extract a named field from a line within a chunk (header or body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkFieldExtract {
    /// Regex pattern with a capture group.
    pub pattern: String,

    /// Variable name for the captured value.
    #[serde(rename = "as")]
    pub as_name: String,

    /// When true, if this field is not extracted from a chunk, it inherits
    /// the value from the most recent chunk that did extract it.
    #[serde(default)]
    pub carry_forward: bool,
}

/// Backward-compatible alias for header extraction.
pub type ChunkExtract = ChunkFieldExtract;

/// Backward-compatible alias for body-line extraction.
pub type ChunkBodyExtract = ChunkFieldExtract;

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
/// interpolated output template. Capture groups use `{1}`, `{2}`, … syntax
/// (tokf-native) or `$1`, `$2` syntax (RTK-compatible). Multiple rules run
/// in order.
///
/// By default, only the first match on each line is replaced and the entire
/// line becomes the interpolated output. When `replace_all = true`, every
/// non-overlapping match on each line is replaced in-place (like
/// `Regex::replace_all`), preserving unmatched portions of the line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceRule {
    pub pattern: String,
    #[serde(alias = "replacement")]
    pub output: String,
    /// When true, replace all non-overlapping matches in each line instead of
    /// replacing the entire line on first match. Default: false.
    #[serde(default)]
    pub replace_all: bool,
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
    /// Regex pattern to match against remaining command-line arguments
    /// (pre-execution detection, Phase A.5). The pattern is tested against
    /// the remaining args joined with spaces.
    pub args_pattern: Option<String>,
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

/// JSON extraction configuration: parse stdout as JSON and extract values via `JSONPath`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonConfig {
    /// Extraction rules to apply to the parsed JSON.
    pub extract: Vec<JsonExtractRule>,
}

/// A single `JSONPath` extraction rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonExtractRule {
    /// `JSONPath` expression (RFC 9535).
    pub path: String,

    /// Variable name to bind the result to.
    #[serde(rename = "as")]
    pub as_name: String,

    /// Optional sub-field extraction for each matched object.
    /// Uses dot-separated paths within each object (not `JSONPath`).
    #[serde(default)]
    pub fields: Vec<JsonFieldExtract>,
}

/// Sub-field extraction within a JSON object matched by a parent rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonFieldExtract {
    /// Dot-separated field path within each matched object (e.g. "metadata.name").
    /// Not a `JSONPath` expression — uses simple dot-notation to traverse nested objects.
    #[serde(alias = "path")]
    pub field: String,

    /// Variable name for the extracted value.
    #[serde(rename = "as")]
    pub as_name: String,
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> FilterConfig {
        toml::from_str(toml).unwrap()
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn passthrough_empty_list_never_triggers() {
        let cfg = parse(r#"command = "gh pr checks *""#);
        assert!(!cfg.should_passthrough(&[s("--watch")]));
    }

    #[test]
    fn passthrough_exact_match() {
        let cfg = parse(
            r#"
command = "gh pr checks *"
passthrough_args = ["--watch", "--web", "-w"]
"#,
        );
        assert!(cfg.should_passthrough(&[s("142"), s("--watch")]));
    }

    #[test]
    fn passthrough_prefix_match() {
        let cfg = parse(
            r#"
command = "docker ps"
passthrough_args = ["--format"]
"#,
        );
        assert!(cfg.should_passthrough(&[s("--format=table")]));
    }

    #[test]
    fn passthrough_short_flag_does_not_match_long() {
        let cfg = parse(
            r#"
command = "gh pr checks *"
passthrough_args = ["--watch"]
"#,
        );
        assert!(!cfg.should_passthrough(&[s("-w")]));
    }

    #[test]
    fn passthrough_no_match_returns_false() {
        let cfg = parse(
            r#"
command = "gh pr checks *"
passthrough_args = ["--watch", "--web"]
"#,
        );
        assert!(!cfg.should_passthrough(&[s("142"), s("--json")]));
    }

    #[test]
    fn passthrough_empty_args_never_triggers() {
        let cfg = parse(
            r#"
command = "gh pr checks *"
passthrough_args = ["--watch"]
"#,
        );
        assert!(!cfg.should_passthrough(&[]));
    }

    #[test]
    fn passthrough_args_deserializes_from_toml() {
        let cfg = parse(
            r#"
command = "gh pr checks *"
passthrough_args = ["--watch", "--web", "-w"]
"#,
        );
        assert_eq!(cfg.passthrough_args, vec!["--watch", "--web", "-w"]);
    }

    #[test]
    fn passthrough_args_defaults_to_empty() {
        let cfg = parse(r#"command = "git push""#);
        assert!(cfg.passthrough_args.is_empty());
    }

    #[test]
    fn passthrough_empty_string_prefix_ignored() {
        let cfg = parse(
            r#"
command = "test"
passthrough_args = [""]
"#,
        );
        assert!(!cfg.should_passthrough(&[s("--anything")]));
    }
}
