use serde::Serialize;

/// A Bash command extracted from a Claude Code session JSONL file.
#[derive(Debug, Clone)]
pub struct ExtractedCommand {
    /// The `tool_use_id` that produced this command.
    pub tool_use_id: String,
    /// The shell command string.
    pub command: String,
    /// Byte length of the paired `tool_result` output (0 if not yet paired).
    pub output_bytes: usize,
}

/// Classification of a single extracted command.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandAnalysis {
    /// Already wrapped with `tokf run` — no action needed.
    AlreadyFiltered,
    /// A matching filter exists; estimated savings percentage.
    Filterable {
        filter_name: String,
        savings_pct: f64,
    },
    /// No matching filter found.
    NoFilter,
}

/// A single aggregated result row: one per unique command pattern.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverResult {
    /// Canonical command pattern (e.g. "git status").
    pub command_pattern: String,
    /// Filter that would handle this command.
    pub filter_name: String,
    /// Number of times this command appeared across sessions.
    pub occurrences: usize,
    /// Total output bytes across all occurrences.
    pub total_output_bytes: usize,
    /// Estimated tokens (`output_bytes / 4`).
    pub estimated_tokens: usize,
    /// Estimated tokens that would be saved.
    pub estimated_savings: usize,
    /// Savings percentage (from tracking history or default).
    pub savings_pct: f64,
}

/// Top-level summary returned by `discover_sessions()`.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverSummary {
    /// Number of session files scanned.
    pub sessions_scanned: usize,
    /// Total Bash commands found.
    pub total_commands: usize,
    /// Commands already filtered by tokf.
    pub already_filtered: usize,
    /// Commands with a matching filter available.
    pub filterable_commands: usize,
    /// Commands with no matching filter.
    pub no_filter_commands: usize,
    /// Estimated total tokens that could be saved.
    pub estimated_total_savings: usize,
    /// Per-command-pattern breakdown, ranked by savings.
    pub results: Vec<DiscoverResult>,
}
