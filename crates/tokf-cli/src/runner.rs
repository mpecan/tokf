use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

/// Re-export `CommandResult` from tokf-filter so existing code that
/// references `crate::runner::CommandResult` continues to work.
pub type CommandResult = tokf_filter::CommandResult;

/// Which stream a line came from.
enum Source {
    Stdout,
    Stderr,
}

/// Extract an exit code from a process status, mapping signals to 128+N on Unix.
fn exit_code_from_status(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .unwrap_or_else(|| status.signal().map_or(1, |s| 128 + s))
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(1)
    }
}

/// Join collected lines into a single string without forcing a trailing newline.
fn join_lines(lines: &[String]) -> String {
    lines.join("\n")
}

/// Run a command, reading stdout and stderr concurrently so that
/// `combined` preserves the real-time interleaving order.
///
/// This is critical for filters that use chunk processing — e.g. the
/// cargo-test filter splits on `Running` headers (stderr) and expects
/// `test result:` lines (stdout) to appear within each chunk.
fn run_interleaved(mut child: std::process::Child) -> anyhow::Result<CommandResult> {
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdout not captured"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("stderr not captured"))?;

    let (tx, rx) = mpsc::channel();
    let tx2 = tx.clone();

    let stdout_thread = thread::spawn(move || {
        let reader = BufReader::new(stdout_pipe);
        for line in reader.lines().map_while(Result::ok) {
            let _ = tx.send((Source::Stdout, line));
        }
    });

    let stderr_thread = thread::spawn(move || {
        let reader = BufReader::new(stderr_pipe);
        for line in reader.lines().map_while(Result::ok) {
            let _ = tx2.send((Source::Stderr, line));
        }
    });

    stdout_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader thread panicked"))?;
    stderr_thread
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader thread panicked"))?;

    // All senders dropped → rx iteration will terminate
    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();
    let mut combined_lines = Vec::new();

    for (source, line) in rx {
        combined_lines.push(line.clone());
        match source {
            Source::Stdout => stdout_lines.push(line),
            Source::Stderr => stderr_lines.push(line),
        }
    }

    let status = child.wait()?;

    Ok(CommandResult {
        stdout: join_lines(&stdout_lines),
        stderr: join_lines(&stderr_lines),
        exit_code: exit_code_from_status(status),
        combined: combined_lines.join("\n"),
    })
}

/// Escape a string for safe inclusion in a shell command (single-quote wrapping).
pub(crate) fn shell_escape(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Execute a command with the given arguments.
///
/// Stdout and stderr are read concurrently so `combined` preserves
/// the real-time interleaving order.
///
/// # Errors
///
/// Returns an error if the command string is empty or the process fails to spawn.
pub fn execute(command: &str, args: &[String]) -> anyhow::Result<CommandResult> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    let base_args: Vec<&str> = parts.collect();

    let child = Command::new(program)
        .args(&base_args)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    run_interleaved(child)
}

/// Execute a shell command with `{args}` interpolation.
///
/// Stdout and stderr are read concurrently so `combined` preserves
/// the real-time interleaving order.
///
/// # Errors
///
/// Returns an error if the shell process fails to spawn.
pub fn execute_shell(run: &str, args: &[String]) -> anyhow::Result<CommandResult> {
    let joined_args = args
        .iter()
        .map(|a| shell_escape(a))
        .collect::<Vec<_>>()
        .join(" ");
    #[allow(clippy::literal_string_with_formatting_args)]
    let shell_cmd = run.replace("{args}", &joined_args);

    let child = Command::new("sh")
        .arg("-c")
        .arg(&shell_cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    run_interleaved(child)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::literal_string_with_formatting_args
)]
mod tests {
    use super::*;

    // --- execute tests ---

    #[test]
    fn test_execute_echo() {
        let result = execute("echo hello", &[]).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn test_execute_with_args() {
        let args = vec!["hello".to_string(), "world".to_string()];
        let result = execute("echo", &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
    }

    #[test]
    fn test_execute_embedded_and_extra_args() {
        let args = vec!["world".to_string()];
        let result = execute("echo hello", &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
    }

    #[test]
    fn test_execute_failure() {
        let result = execute("false", &[]).unwrap();
        assert_ne!(result.exit_code, 0);
    }

    #[test]
    fn test_execute_specific_exit_code() {
        let result = execute_shell("exit 42", &[]).unwrap();
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn test_execute_empty_command() {
        let result = execute("", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_whitespace_only_command() {
        let result = execute("   ", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_nonexistent_command() {
        let result = execute("nonexistent_cmd_xyz", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_args_with_special_characters() {
        // execute() uses Command::new (no shell), so special chars are passed literally
        let args = vec!["hello world".to_string()];
        let result = execute("echo", &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
        assert_eq!(result.exit_code, 0);
    }

    // --- execute_shell tests ---

    #[test]
    fn test_execute_shell_basic() {
        let result = execute_shell("echo hello", &[]).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn test_execute_shell_args_interpolation() {
        let args = vec!["a".to_string(), "b".to_string()];
        let result = execute_shell("echo {args}", &args).unwrap();
        assert_eq!(result.stdout.trim(), "a b");
    }

    #[test]
    fn test_execute_shell_args_empty() {
        let result = execute_shell("echo {args} done", &[]).unwrap();
        assert_eq!(result.stdout.trim(), "done");
    }

    #[test]
    fn test_execute_shell_args_escaped() {
        let args = vec!["hello world".to_string()];
        let result = execute_shell("echo {args}", &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
    }

    #[test]
    fn test_execute_shell_args_with_semicolon() {
        let args = vec!["; echo injected".to_string()];
        let result = execute_shell("echo {args}", &args).unwrap();
        let stdout = result.stdout.trim();
        // The semicolon should be escaped and printed literally, not executed
        assert!(stdout.contains("; echo injected"));
        // "injected" should not appear as a separate execution
        assert!(!stdout.contains("\ninjected"));
    }

    // --- build_result / combined field tests ---

    #[test]
    fn test_execute_stderr() {
        let result = execute_shell("echo err >&2", &[]).unwrap();
        assert!(result.stderr.contains("err"));
        assert!(result.stdout.is_empty());
        assert_eq!(result.combined, "err");
    }

    #[test]
    fn test_combined_both_empty() {
        let result = execute("true", &[]).unwrap();
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
        assert_eq!(result.combined, "");
    }

    #[test]
    fn test_combined_stdout_only() {
        let result = execute("echo hello", &[]).unwrap();
        assert_eq!(result.combined, "hello");
    }

    #[test]
    fn test_combined_stderr_only() {
        let result = execute_shell("echo err >&2", &[]).unwrap();
        assert_eq!(result.combined, "err");
    }

    #[test]
    fn test_combined_both_streams() {
        let result = execute_shell("echo out && echo err >&2", &[]).unwrap();
        // Both streams present in combined; exact order depends on scheduling
        assert!(result.combined.contains("out"));
        assert!(result.combined.contains("err"));
    }

    #[test]
    fn test_combined_interleaving() {
        // Verify that stderr lines appear interleaved with stdout, not appended
        let result = execute_shell(
            "echo out1 && echo err1 >&2 && echo out2 && echo err2 >&2",
            &[],
        )
        .unwrap();
        assert!(result.combined.contains("out1"));
        assert!(result.combined.contains("out2"));
        assert!(result.combined.contains("err1"));
        assert!(result.combined.contains("err2"));
        assert!(result.stdout.contains("out1"));
        assert!(result.stdout.contains("out2"));
        assert!(result.stderr.contains("err1"));
        assert!(result.stderr.contains("err2"));
    }

    // --- signal handling (unix only) ---

    #[cfg(unix)]
    #[test]
    fn test_execute_signal_exit_code() {
        // SIGTERM = 15, expected exit code = 128 + 15 = 143
        let result = execute_shell("kill -TERM $$", &[]).unwrap();
        assert_eq!(result.exit_code, 143);
    }
}
