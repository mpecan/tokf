#![allow(dead_code)]

use std::collections::HashMap;

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
    pub labels: HashMap<String, String>,
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn load_filter(name: &str) -> FilterConfig {
        let path = format!("{}/filters/{name}", env!("CARGO_MANIFEST_DIR"));
        let content = std::fs::read_to_string(&path).unwrap();
        toml::from_str(&content).unwrap()
    }

    // --- CommandPattern deserialization ---

    #[test]
    fn test_command_pattern_single() {
        let cfg: FilterConfig = toml::from_str(r#"command = "git push""#).unwrap();
        assert_eq!(cfg.command, CommandPattern::Single("git push".to_string()));
        assert_eq!(cfg.command.first(), "git push");
        assert_eq!(cfg.command.patterns(), &["git push".to_string()]);
    }

    #[test]
    fn test_command_pattern_multiple() {
        let cfg: FilterConfig = toml::from_str(r#"command = ["pnpm test", "npm test"]"#).unwrap();
        assert_eq!(
            cfg.command,
            CommandPattern::Multiple(vec!["pnpm test".to_string(), "npm test".to_string()])
        );
        assert_eq!(cfg.command.first(), "pnpm test");
        assert_eq!(
            cfg.command.patterns(),
            &["pnpm test".to_string(), "npm test".to_string()]
        );
    }

    #[test]
    fn test_command_pattern_wildcard() {
        let cfg: FilterConfig = toml::from_str(r#"command = "npm run *""#).unwrap();
        assert_eq!(cfg.command.first(), "npm run *");
    }

    // --- Stdlib filter deserialization ---

    #[test]
    fn test_deserialize_git_push() {
        let cfg = load_filter("git/push.toml");

        assert_eq!(cfg.command.first(), "git push");
        assert_eq!(cfg.match_output.len(), 2);
        assert_eq!(cfg.match_output[0].contains, "Everything up-to-date");
        assert_eq!(cfg.match_output[1].contains, "rejected");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.skip.len(), 8);
        assert!(success.skip[0].starts_with("^Enumerating"));

        let extract = success.extract.unwrap();
        assert!(extract.pattern.contains("->"));
        assert_eq!(extract.output, "ok \u{2713} {2}");

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(10));
    }

    #[test]
    fn test_deserialize_git_status() {
        let cfg = load_filter("git/status.toml");

        assert_eq!(cfg.command.first(), "git status");
        assert_eq!(cfg.run.as_deref(), Some("git status --porcelain -b"));

        let parse = cfg.parse.unwrap();
        let branch = parse.branch.unwrap();
        assert_eq!(branch.line, 1);
        assert_eq!(branch.output, "{1}");

        let group = parse.group.unwrap();
        assert!(group.labels.contains_key("??"));
        assert_eq!(group.labels.get("M ").unwrap(), "modified");

        let output = cfg.output.unwrap();
        assert!(output.format.unwrap().contains("{branch}"));
        assert_eq!(
            output.group_counts_format.as_deref(),
            Some("  {label}: {count}")
        );
        assert_eq!(
            output.empty.as_deref(),
            Some("clean \u{2014} nothing to commit")
        );
    }

    #[test]
    fn test_deserialize_cargo_test() {
        let cfg = load_filter("cargo/test.toml");

        assert_eq!(cfg.command.first(), "cargo test");
        assert!(!cfg.skip.is_empty());
        assert!(cfg.skip.iter().any(|s| s.contains("Compiling")));

        assert_eq!(cfg.section.len(), 3);
        assert_eq!(cfg.section[0].name.as_deref(), Some("failures"));
        assert_eq!(cfg.section[0].collect_as.as_deref(), Some("failure_blocks"));
        assert_eq!(cfg.section[1].name.as_deref(), Some("failure_names"));
        assert_eq!(cfg.section[2].name.as_deref(), Some("summary"));

        let success = cfg.on_success.unwrap();
        let agg = success.aggregate.unwrap();
        assert_eq!(agg.from, "summary_lines");
        assert_eq!(agg.sum.as_deref(), Some("passed"));
        assert_eq!(agg.count_as.as_deref(), Some("suites"));
        assert!(success.output.unwrap().contains("{passed}"));

        let failure = cfg.on_failure.unwrap();
        assert!(failure.output.unwrap().contains("FAILURES"));

        let fallback = cfg.fallback.unwrap();
        assert_eq!(fallback.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_add() {
        let cfg = load_filter("git/add.toml");

        assert_eq!(cfg.command.first(), "git add");
        assert_eq!(cfg.match_output.len(), 1);
        assert_eq!(cfg.match_output[0].contains, "fatal:");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("ok \u{2713}"));

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_commit() {
        let cfg = load_filter("git/commit.toml");

        assert_eq!(cfg.command.first(), "git commit");

        let success = cfg.on_success.unwrap();
        let extract = success.extract.unwrap();
        assert!(extract.pattern.contains("\\w+"));
        assert_eq!(extract.output, "ok \u{2713} {2}");

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_log() {
        let cfg = load_filter("git/log.toml");

        assert_eq!(cfg.command.first(), "git log");

        let run = cfg.run.unwrap();
        assert!(run.contains("{args}"));
        assert!(run.contains("--oneline"));

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("{output}"));
    }

    #[test]
    fn test_deserialize_git_diff() {
        let cfg = load_filter("git/diff.toml");

        assert_eq!(cfg.command.first(), "git diff");

        let run = cfg.run.unwrap();
        assert!(run.contains("--stat"));
        assert!(run.contains("{args}"));

        assert_eq!(cfg.match_output.len(), 1);
        assert_eq!(cfg.match_output[0].contains, "fatal:");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("{output}"));

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    // --- Minimal / defaults ---

    #[test]
    fn test_minimal_config_only_command() {
        let cfg: FilterConfig = toml::from_str(r#"command = "echo""#).unwrap();

        assert_eq!(cfg.command.first(), "echo");
        assert_eq!(cfg.run, None);
        assert!(cfg.skip.is_empty());
        assert!(cfg.keep.is_empty());
        assert!(cfg.step.is_empty());
        assert_eq!(cfg.extract, None);
        assert!(cfg.match_output.is_empty());
        assert!(cfg.section.is_empty());
        assert_eq!(cfg.on_success, None);
        assert_eq!(cfg.on_failure, None);
        assert_eq!(cfg.parse, None);
        assert_eq!(cfg.output, None);
        assert_eq!(cfg.fallback, None);
        assert!(cfg.replace.is_empty());
        assert!(!cfg.dedup);
        assert_eq!(cfg.dedup_window, None);
        assert!(!cfg.strip_ansi);
        assert!(!cfg.trim_lines);
        assert!(!cfg.strip_empty_lines);
        assert!(!cfg.collapse_empty_lines);
        assert_eq!(cfg.lua_script, None);
        assert!(cfg.variant.is_empty());
    }

    // --- Variant deserialization ---

    #[test]
    fn test_variant_with_file_detection() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = ["npm test", "pnpm test"]

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts", "vitest.config.js"]
filter = "npm/test-vitest"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 1);
        assert_eq!(cfg.variant[0].name, "vitest");
        assert_eq!(
            cfg.variant[0].detect.files,
            vec!["vitest.config.ts", "vitest.config.js"]
        );
        assert_eq!(cfg.variant[0].detect.output_pattern, None);
        assert_eq!(cfg.variant[0].filter, "npm/test-vitest");
    }

    #[test]
    fn test_variant_with_output_pattern() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = "npm test"

[[variant]]
name = "mocha"
detect.output_pattern = "passing|failing|pending"
filter = "npm/test-mocha"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 1);
        assert_eq!(cfg.variant[0].name, "mocha");
        assert!(cfg.variant[0].detect.files.is_empty());
        assert_eq!(
            cfg.variant[0].detect.output_pattern.as_deref(),
            Some("passing|failing|pending")
        );
        assert_eq!(cfg.variant[0].filter, "npm/test-mocha");
    }

    #[test]
    fn test_multiple_variants() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = "npm test"

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts"]
filter = "npm/test-vitest"

[[variant]]
name = "jest"
detect.files = ["jest.config.js"]
filter = "npm/test-jest"

[[variant]]
name = "mocha"
detect.output_pattern = "passing|failing"
filter = "npm/test-mocha"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 3);
        assert_eq!(cfg.variant[0].name, "vitest");
        assert_eq!(cfg.variant[1].name, "jest");
        assert_eq!(cfg.variant[2].name, "mocha");
    }

    // --- Negative tests ---

    #[test]
    fn test_missing_command_field_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(r#"run = "echo hello""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_type_for_skip_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(
            r#"command = "echo"
skip = "not-an-array""#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_type_for_tail_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(
            r#"command = "echo"
[on_success]
tail = "five""#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_toml_fails() {
        let result: Result<FilterConfig, _> = toml::from_str("command = [unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_toml_fails() {
        let result: Result<FilterConfig, _> = toml::from_str("");
        assert!(result.is_err());
    }
}
