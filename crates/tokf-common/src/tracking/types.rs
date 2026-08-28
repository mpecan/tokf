#[derive(Debug)]
pub struct TrackingEvent {
    pub command: String,
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub input_bytes: i64,
    pub output_bytes: i64,
    pub input_tokens_est: i64,
    pub output_tokens_est: i64,
    /// Raw command output bytes before any baseline adjustment.
    pub raw_bytes: i64,
    /// Estimated raw tokens (see [`crate::tokens`]).
    pub raw_tokens_est: i64,
    pub filter_time_ms: i64,
    pub exit_code: i32,
    /// True when `--prefer-less` chose the piped output over the filtered output.
    pub pipe_override: bool,
    /// For a captured pipeline, everything after the first bare pipe (the part
    /// tokf ran on the command's behalf). `None` for every other run — which is
    /// what lets `gain`/`doctor` tell capture rows apart from real passthroughs
    /// rather than lumping both under `COALESCE(filter_name, 'passthrough')`.
    pub pipeline_tail: Option<String>,
    /// For a captured pipeline, the *first stage's* exit code. `exit_code`
    /// stays the shell-native one, so `head_exit_code != exit_code` is exactly
    /// the swallowed-status signal.
    pub head_exit_code: Option<i32>,
    /// Project identifier — typically the cwd's directory name when the
    /// event was recorded. Empty string means "unknown" (legacy events
    /// recorded before this column existed, or test fixtures).
    pub project: String,
}

#[derive(serde::Serialize)]
pub struct GainSummary {
    pub total_commands: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub tokens_saved: i64,
    pub savings_pct: f64,
    pub pipe_override_count: i64,
    /// Captured pipelines where the command's exit code and the code the shell
    /// reported disagreed — i.e. the pipeline hid a failure.
    pub exit_mismatch_count: i64,
    pub total_filter_time_ms: i64,
    pub avg_filter_time_ms: f64,
    /// Total raw tokens intercepted (before baseline adjustment).
    pub total_raw_tokens: i64,
}

#[derive(serde::Serialize)]
pub struct DailyGain {
    pub date: String,
    pub commands: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tokens_saved: i64,
    pub savings_pct: f64,
    pub pipe_override_count: i64,
    pub total_filter_time_ms: i64,
    pub raw_tokens: i64,
}

#[derive(Clone, serde::Serialize)]
pub struct FilterGain {
    pub filter_name: String,
    pub commands: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tokens_saved: i64,
    pub savings_pct: f64,
    pub pipe_override_count: i64,
    pub total_filter_time_ms: i64,
    pub avg_filter_time_ms: f64,
    pub raw_tokens: i64,
}
