//! Compute the "fair baseline" byte count by piping raw output through
//! the original pipe command the user would have used without tokf.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Allowed first words for baseline pipe commands (security whitelist).
const ALLOWED_COMMANDS: &[&str] = &["tail", "head", "grep"];

/// Maximum time to wait for the baseline pipe command before falling back.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Run the pipe command on the raw output to get the exact byte count
/// the user would have seen without tokf.
///
/// Only allows `tail`, `head`, and `grep` as pipe commands (matching the
/// rewrite module's strippable set). Falls back to `raw_output.len()` with
/// a stderr warning on validation failure, spawn failure, or timeout.
pub fn compute(raw_output: &str, pipe_cmd: &str) -> usize {
    let first_word = pipe_cmd.split_whitespace().next().unwrap_or("");
    if !ALLOWED_COMMANDS.contains(&first_word) {
        eprintln!(
            "[tokf] warning: --baseline-pipe command '{first_word}' not allowed, using full output"
        );
        return raw_output.len();
    }

    let Ok(mut child) = Command::new("sh")
        .args(["-c", pipe_cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("[tokf] warning: --baseline-pipe failed to spawn, using full output");
        return raw_output.len();
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
                    Ok(output) => output.stdout.len(),
                    Err(e) => {
                        eprintln!(
                            "[tokf] warning: --baseline-pipe read failed: {e}, using full output"
                        );
                        raw_output.len()
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
                    return raw_output.len();
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("[tokf] warning: --baseline-pipe wait failed: {e}, using full output");
                return raw_output.len();
            }
        }
    }
}
