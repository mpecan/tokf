use std::process::Command;

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub combined: String,
}

fn build_result(output: &std::process::Output) -> CommandResult {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    #[cfg(unix)]
    let exit_code = {
        use std::os::unix::process::ExitStatusExt;
        output
            .status
            .code()
            .unwrap_or_else(|| output.status.signal().map_or(1, |s| 128 + s))
    };
    #[cfg(not(unix))]
    let exit_code = output.status.code().unwrap_or(1);

    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.clone(),
        (true, false) => stderr.clone(),
        (false, false) => format!("{}\n{}", stdout.trim_end(), stderr),
    };
    let combined = combined.trim_end().to_string();

    CommandResult {
        stdout,
        stderr,
        exit_code,
        combined,
    }
}

/// Escape a string for safe inclusion in a shell command (single-quote wrapping).
pub(crate) fn shell_escape(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Execute a command with the given arguments.
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

    let output = Command::new(program).args(&base_args).args(args).output()?;

    Ok(build_result(&output))
}

/// Execute a shell command with `{args}` interpolation.
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

    let output = Command::new("sh").arg("-c").arg(&shell_cmd).output()?;

    Ok(build_result(&output))
}

#[cfg(test)]
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
        assert_eq!(result.combined, "out\nerr");
    }

    #[test]
    fn test_combined_no_double_newline() {
        // stdout from echo ends with \n; combined should not have a blank line between streams
        let result = execute_shell("echo out && echo err >&2", &[]).unwrap();
        assert!(!result.combined.contains("\n\n"));
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
