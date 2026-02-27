pub mod filter;
pub mod verify;

/// The result of executing a command, used as input to the filter pipeline.
///
/// This struct contains only the data needed for filtering — it does not
/// include process execution logic (which lives in tokf-cli's `runner` module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub combined: String,
}
