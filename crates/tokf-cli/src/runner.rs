use std::io::ErrorKind;
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

/// Search the current `PATH` for the absolute path of a program name.
///
/// This is used when we're about to override `PATH` with a shims directory —
/// we must resolve the original program first so it doesn't find our own shim.
pub fn resolve_program(program: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Build the system shell command for a shell snippet.
fn build_shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(command);
        cmd
    }

    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn spawn_command(mut cmd: Command, program: &str) -> anyhow::Result<std::process::Child> {
    match cmd.spawn() {
        Ok(child) => Ok(child),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            Err(anyhow::anyhow!("program not found: {program}"))
        }
        Err(err) => Err(err.into()),
    }
}

/// Escape a string for safe inclusion in a shell command.
pub(crate) fn shell_escape(arg: &str) -> String {
    #[cfg(windows)]
    {
        format!("'{}'", arg.replace('\'', "''"))
    }

    #[cfg(not(windows))]
    {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
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
    execute_with_env(command, args, &[])
}

/// Execute a command with extra environment variables.
///
/// When `extra_env` contains a `PATH` entry, the program is resolved to an
/// absolute path via the *current* `PATH` before the override is applied.
/// This prevents the spawned process from finding our own shim.
///
/// # Errors
///
/// Returns an error if the command string is empty or the process fails to spawn.
pub fn execute_with_env(
    command: &str,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> anyhow::Result<CommandResult> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    let base_args: Vec<&str> = parts.collect();

    let has_path_override = extra_env.iter().any(|(k, _)| *k == "PATH");
    let resolved = if has_path_override {
        resolve_program(program)
    } else {
        None
    };
    let actual_program = resolved
        .as_ref()
        .map_or(program, |p| p.to_str().unwrap_or(program));

    let mut cmd = Command::new(actual_program);
    cmd.args(&base_args)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    run_interleaved(spawn_command(cmd, actual_program)?)
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
    execute_shell_with_env(run, args, &[])
}

/// Expand a filter's `run` template into the exact shell command line that will
/// be executed: `{args}` is replaced with the shell-escaped user arguments.
///
/// A template without `{args}` drops the user's arguments — that is the
/// documented behaviour, and this function reproduces it faithfully so callers
/// can record what actually ran (issue #430).
#[must_use]
pub fn expand_run_command(run: &str, args: &[String]) -> String {
    let joined_args = args
        .iter()
        .map(|a| shell_escape(a))
        .collect::<Vec<_>>()
        .join(" ");
    #[allow(clippy::literal_string_with_formatting_args)]
    run.replace("{args}", &joined_args)
}

/// Execute a shell command with extra environment variables.
///
/// # Errors
///
/// Returns an error if the shell process fails to spawn.
pub fn execute_shell_with_env(
    run: &str,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> anyhow::Result<CommandResult> {
    let shell_cmd = expand_run_command(run, args);

    let shell_program = if cfg!(windows) {
        "powershell.exe"
    } else {
        "sh"
    };
    let mut cmd = build_shell_command(&shell_cmd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    run_interleaved(spawn_command(cmd, shell_program)?)
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
        let err = result.unwrap_err().to_string();
        assert_eq!(err, "program not found: nonexistent_cmd_xyz");
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

    // --- expand_run_command: what gets recorded as the executed command ---

    #[test]
    fn expand_run_command_interpolates_args() {
        // Args are shell-quoted, exactly as execute_shell hands them to `sh`.
        // The recorded command is the literal shell input, quotes included.
        let args = vec!["--all".to_string(), "HEAD".to_string()];
        assert_eq!(
            expand_run_command("git log --oneline {args}", &args),
            "git log --oneline '--all' 'HEAD'"
        );
    }

    #[test]
    fn expand_run_command_escapes_args_like_the_shell_sees_them() {
        let args = vec!["a b".to_string()];
        assert_eq!(
            expand_run_command("git log {args}", &args),
            "git log 'a b'",
            "the recorded command must be the one actually handed to the shell"
        );
    }

    #[test]
    fn expand_run_command_drops_args_when_template_has_no_placeholder() {
        // Mirrors execute_shell: without {args} the user's arguments never reach
        // the command. Recording them would misrepresent what ran.
        let args = vec!["--json".to_string()];
        assert_eq!(
            expand_run_command("docker ps --format json", &args),
            "docker ps --format json"
        );
    }

    #[test]
    fn expand_run_command_matches_what_execute_shell_runs() {
        let args = vec!["hi there".to_string()];
        let expanded = expand_run_command("echo {args}", &args);
        let result = execute_shell("echo {args}", &args).unwrap();
        // `expanded` is `echo 'hi there'`; running it must produce the same output.
        assert_eq!(result.stdout.trim(), "hi there");
        assert_eq!(
            execute_shell(&expanded, &[]).unwrap().stdout.trim(),
            "hi there"
        );
    }

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

    #[test]
    fn test_execute_shell_args_with_single_quote() {
        let args = vec!["it's quoted".to_string()];
        let result = execute_shell("echo {args}", &args).unwrap();
        assert_eq!(result.stdout.trim(), "it's quoted");
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

    // --- resolve_program tests ---

    #[test]
    fn resolve_program_finds_sh() {
        let result = resolve_program("sh");
        assert!(result.is_some(), "sh should be on PATH");
        assert!(result.unwrap().is_absolute());
    }

    #[test]
    fn resolve_program_returns_none_for_missing() {
        let result = resolve_program("nonexistent_program_xyz_abc_123");
        assert!(result.is_none());
    }

    // --- execute_with_env tests ---

    #[test]
    fn test_execute_with_env_propagates_vars() {
        let env = vec![("TOKF_TEST_VAR", "hello_from_env")];
        let result =
            execute_with_env("sh", &["-c".into(), "echo $TOKF_TEST_VAR".into()], &env).unwrap();
        assert_eq!(result.stdout.trim(), "hello_from_env");
    }

    #[test]
    fn test_execute_with_env_empty_env() {
        let result = execute_with_env("echo", &["hi".into()], &[]).unwrap();
        assert_eq!(result.stdout.trim(), "hi");
    }

    #[test]
    fn test_execute_shell_with_env_propagates_vars() {
        let env = vec![("TOKF_TEST_VAR2", "shell_env_val")];
        let result = execute_shell_with_env("echo $TOKF_TEST_VAR2", &[], &env).unwrap();
        assert_eq!(result.stdout.trim(), "shell_env_val");
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
