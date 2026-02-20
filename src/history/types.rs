/// A single history entry recording both raw and filtered output
#[derive(Debug)]
pub struct HistoryEntry {
    pub id: i64,
    pub timestamp: String,
    pub project: String,
    pub command: String,
    pub filter_name: Option<String>,
    pub raw_output: String,
    pub filtered_output: String,
    pub exit_code: i32,
}

/// Parameters for recording one history entry.
pub struct HistoryRecord {
    pub project: String,
    pub command: String,
    pub filter_name: Option<String>,
    pub raw_output: String,
    pub filtered_output: String,
    pub exit_code: i32,
}
