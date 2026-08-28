//! Compute the "fair baseline" byte count by piping raw output through
//! the original pipe command the user would have used without tokf.

use std::time::Duration;

use crate::pipe_exec::{self, PipeSpec, Stderr};

/// A baseline runs on a sample of already-captured output, so it should be
/// quick; a slow one means something is wrong, not that it needs longer.
const BASELINE_TIMEOUT: Duration = Duration::from_secs(5);

/// Allowed first words for baseline pipe commands (security whitelist).
const ALLOWED_COMMANDS: &[&str] = &["tail", "head", "grep"];

/// Run the pipe command on the raw output and return the actual text the
/// user would have seen.
///
/// Only allows `tail`, `head`, and `grep` as pipe commands (matching the
/// rewrite module's strippable set). Returns `None` on validation failure,
/// spawn failure, timeout, or read error — callers should fall back to
/// `raw_output` in that case.
pub fn compute_output(raw_output: &str, pipe_cmd: &str) -> Option<String> {
    let first_word = pipe_cmd.split_whitespace().next().unwrap_or("");
    if !ALLOWED_COMMANDS.contains(&first_word) {
        eprintln!(
            "[tokf] warning: --baseline-pipe command '{first_word}' not allowed, using full output"
        );
        return None;
    }

    // stderr is discarded: this is an invisible accounting run, and leaking the
    // measurement subprocess's stderr into the terminal would be noise the user
    // never asked for.
    let spec = PipeSpec {
        timeout: BASELINE_TIMEOUT,
        stderr: Stderr::Discard,
    };
    match pipe_exec::run(raw_output, pipe_cmd, spec) {
        Ok(out) => Some(out.stdout),
        Err(e) => {
            eprintln!("[tokf] warning: --baseline-pipe {e}, using full output");
            None
        }
    }
}

/// Run the pipe command on the raw output to get the exact byte count
/// the user would have seen without tokf.
///
/// Falls back to `raw_output.len()` when the pipe command fails.
pub fn compute(raw_output: &str, pipe_cmd: &str) -> usize {
    compute_output(raw_output, pipe_cmd).map_or(raw_output.len(), |s| s.len())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn compute_output_tail() {
        let input = "line1\nline2\nline3\nline4\nline5\n";
        let result = compute_output(input, "tail -2").unwrap();
        assert_eq!(result, "line4\nline5\n");
    }

    #[test]
    fn compute_output_head() {
        let input = "line1\nline2\nline3\nline4\nline5\n";
        let result = compute_output(input, "head -2").unwrap();
        assert_eq!(result, "line1\nline2\n");
    }

    #[test]
    fn compute_output_grep() {
        let input = "apple\nbanana\napricot\ncherry\n";
        let result = compute_output(input, "grep ap").unwrap();
        assert_eq!(result, "apple\napricot\n");
    }

    #[test]
    fn compute_output_disallowed_command() {
        let result = compute_output("data", "rm -rf /");
        assert!(result.is_none());
    }

    #[test]
    fn compute_delegates_to_compute_output() {
        let input = "line1\nline2\nline3\n";
        let bytes = compute(input, "head -1");
        assert_eq!(bytes, "line1\n".len());
    }

    #[test]
    fn compute_fallback_on_disallowed() {
        let input = "some data";
        let bytes = compute(input, "cat");
        assert_eq!(bytes, input.len());
    }
}
