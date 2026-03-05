//! Compute the "fair baseline" byte count by piping raw output through
//! the original pipe command the user would have used without tokf.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Allowed first words for baseline pipe commands (security whitelist).
const ALLOWED_COMMANDS: &[&str] = &["tail", "head", "grep"];

/// Maximum time to wait for the baseline pipe command before falling back.
const TIMEOUT: Duration = Duration::from_secs(5);

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

    let Ok(mut child) = Command::new("sh")
        .args(["-c", pipe_cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("[tokf] warning: --baseline-pipe failed to spawn, using full output");
        return None;
    };

    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(raw_output.as_bytes());
    }
    drop(child.stdin.take());

    // Wait with timeout to prevent hanging on misbehaving pipe commands.
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                return match child.wait_with_output() {
                    Ok(output) => String::from_utf8(output.stdout).ok(),
                    Err(e) => {
                        eprintln!(
                            "[tokf] warning: --baseline-pipe read failed: {e}, using full output"
                        );
                        None
                    }
                };
            }
            Ok(None) => {
                if start.elapsed() >= TIMEOUT {
                    let _ = child.kill();
                    eprintln!(
                        "[tokf] warning: --baseline-pipe timed out after {}s, using full output",
                        TIMEOUT.as_secs()
                    );
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("[tokf] warning: --baseline-pipe wait failed: {e}, using full output");
                return None;
            }
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
