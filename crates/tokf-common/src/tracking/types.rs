#[derive(Debug)]
pub struct TrackingEvent {
    pub command: String,
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub input_bytes: i64,
    pub output_bytes: i64,
    pub input_tokens_est: i64,
    pub output_tokens_est: i64,
    pub filter_time_ms: i64,
    pub exit_code: i32,
    /// True when `--prefer-less` chose the piped output over the filtered output.
    pub pipe_override: bool,
}

#[derive(serde::Serialize)]
pub struct GainSummary {
    pub total_commands: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub tokens_saved: i64,
    pub savings_pct: f64,
    pub pipe_override_count: i64,
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
}

#[derive(serde::Serialize)]
pub struct FilterGain {
    pub filter_name: String,
    pub commands: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tokens_saved: i64,
    pub savings_pct: f64,
    pub pipe_override_count: i64,
}
