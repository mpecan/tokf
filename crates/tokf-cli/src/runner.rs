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
///
/// Delegates to `which`, which applies the platform's own resolution rules.
/// That matters on Windows, where a bare name is resolved through `PATHEXT`:
/// a hand-rolled `dir.join(program)` loop had no success mode there. For `npm`
/// it matched the extensionless POSIX shell script that ships next to
/// `npm.cmd` — a file `CreateProcessW` cannot execute — and for a normal `.exe`
/// such as `git` it found nothing at all, so the shim-avoidance this function
/// exists for never happened.
pub fn resolve_program(program: &str) -> Option<std::path::PathBuf> {
    which::which(program).ok()
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

/// Spawn a prepared command, leaving the `io::Error` intact.
///
/// The raw error kind is preserved so callers can distinguish `NotFound` and
/// retry with a differently-resolved program path.
fn spawn_command(mut cmd: Command) -> std::io::Result<std::process::Child> {
    cmd.spawn()
}

/// Spawn a prepared command, reporting a missing program by name.
fn spawn_named(cmd: Command, program: &str) -> anyhow::Result<std::process::Child> {
    spawn_command(cmd).map_err(|err| match err.kind() {
        ErrorKind::NotFound => anyhow::anyhow!("program not found: {program}"),
        _ => err.into(),
    })
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
    program: &str,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> anyhow::Result<CommandResult> {
    if program.is_empty() {
        anyhow::bail!("empty command");
    }

    let has_path_override = extra_env.iter().any(|(k, _)| *k == "PATH");
    let resolved = if has_path_override {
        resolve_program(program)
    } else {
        None
    };
    let actual_program = resolved
        .as_ref()
        .map_or(program, |p| p.to_str().unwrap_or(program));

    match spawn_command(build_command(actual_program, args, extra_env)) {
        Ok(child) => run_interleaved(child),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // Windows resolves a bare program name through CreateProcessW,
            // which only ever appends `.exe`. Every other PATHEXT entry —
            // `.cmd`, `.bat`, `.com`, … — is unreachable, so `npm` fails while
            // `npm.cmd` works. `which` applies PATHEXT, so retry through it
            // once before giving up. On Unix this second attempt simply finds
            // nothing new and the original error stands.
            let fallback = resolve_program(actual_program)
                .filter(|p| p.as_os_str() != actual_program)
                .ok_or_else(|| anyhow::anyhow!("program not found: {actual_program}"))?;

            let child = spawn_named(
                build_command(&fallback.to_string_lossy(), args, extra_env),
                actual_program,
            )?;
            run_interleaved(child)
        }
        Err(err) => Err(err.into()),
    }
}

/// Build the child process for `program`, without spawning it.
///
/// Separate from the spawn so a failed attempt can be rebuilt and retried with
/// a different program path — `Command` does not allow changing it after the
/// fact.
fn build_command(program: &str, args: &[String], extra_env: &[(&str, &str)]) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd
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

    run_interleaved(spawn_named(cmd, shell_program)?)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::literal_string_with_formatting_args
)]
mod tests {
    use super::*;

    // --- cross-platform process helpers ---
    //
    // These tests spawn real processes, and the usual Unix stand-ins do not
    // exist on Windows: `echo` and `false` are `cmd.exe` builtins with no
    // executable behind them, so `Command::new("echo")` genuinely finds
    // nothing. Routing through `cmd /C` keeps the same assertions meaningful on
    // both platforms rather than compiling the coverage out on one of them.

    /// Program + leading args that echo `words` back on the current platform.
    fn echo_program(words: &[&str]) -> (&'static str, Vec<String>) {
        let (program, mut args) = if cfg!(windows) {
            ("cmd", vec!["/C".to_string(), "echo".to_string()])
        } else {
            ("echo", Vec::new())
        };
        args.extend(words.iter().map(|w| (*w).to_string()));
        (program, args)
    }

    /// Program + args that exit non-zero without printing anything.
    fn failing_program() -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            ("cmd", vec!["/C".to_string(), "exit 1".to_string()])
        } else {
            ("false", Vec::new())
        }
    }

    /// A shell snippet writing `msg` to stderr, in the platform shell's own
    /// syntax — `execute_shell` uses `sh` on Unix and `powershell.exe` on
    /// Windows, where `>&2` is not a redirection.
    fn write_stderr(msg: &str) -> String {
        if cfg!(windows) {
            format!("[Console]::Error.WriteLine('{msg}')")
        } else {
            format!("echo {msg} >&2")
        }
    }

    /// Join shell statements so they run in order. Windows PowerShell 5.1 — the
    /// `powershell.exe` on the runners — has no `&&`.
    fn shell_seq(parts: &[String]) -> String {
        parts.join(if cfg!(windows) { "; " } else { " && " })
    }

    // --- execute tests ---

    #[test]
    fn test_execute_echo() {
        let (program, args) = echo_program(&["hello"]);
        let result = execute(program, &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn test_execute_with_args() {
        let (program, args) = echo_program(&["hello", "world"]);
        let result = execute(program, &args).unwrap();
        assert_eq!(result.stdout.trim(), "hello world");
    }

    /// The first argument is a program, never a command line: it must reach
    /// the OS whole. Splitting it on whitespace is what tore
    /// `C:\Program Files\node.exe` in half and reported it as not found (#450).
    #[test]
    fn execute_does_not_split_the_program_on_whitespace() {
        let err = execute("echo hello", &[]).unwrap_err().to_string();
        assert_eq!(
            err, "program not found: echo hello",
            "the whole string must be treated as one program name"
        );
    }

    #[test]
    fn test_execute_failure() {
        let (program, args) = failing_program();
        let result = execute(program, &args).unwrap();
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
        let (program, args) = echo_program(&["hello world"]);
        let result = execute(program, &args).unwrap();
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
        let (program, args) = echo_program(&["hello"]);
        let result = execute(program, &args).unwrap();
        assert_eq!(result.combined, "hello");
    }

    #[test]
    fn test_combined_stderr_only() {
        let result = execute_shell(&write_stderr("err"), &[]).unwrap();
        assert_eq!(result.combined, "err");
    }

    #[test]
    fn test_combined_both_streams() {
        let script = shell_seq(&["echo out".to_string(), write_stderr("err")]);
        let result = execute_shell(&script, &[]).unwrap();
        // Both streams present in combined; exact order depends on scheduling
        assert!(result.combined.contains("out"));
        assert!(result.combined.contains("err"));
    }

    #[test]
    fn test_combined_interleaving() {
        // Verify that stderr lines appear interleaved with stdout, not appended
        let script = shell_seq(&[
            "echo out1".to_string(),
            write_stderr("err1"),
            "echo out2".to_string(),
            write_stderr("err2"),
        ]);
        let result = execute_shell(&script, &[]).unwrap();
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

    /// A program every platform has, resolved by bare name.
    ///
    /// This failed on Windows before the switch to `which`: the old
    /// `dir.join(program)` loop never appended an extension, so no `.exe` was
    /// ever found by its bare name.
    #[test]
    fn resolve_program_finds_a_shell_by_bare_name() {
        let name = if cfg!(windows) { "cmd" } else { "sh" };
        let result = resolve_program(name);
        assert!(result.is_some(), "{name} should be on PATH");
        assert!(result.unwrap().is_absolute());
    }

    #[test]
    fn resolve_program_returns_none_for_missing() {
        let result = resolve_program("nonexistent_program_xyz_abc_123");
        assert!(result.is_none());
    }

    /// Windows resolves a bare program name through `PATHEXT`, but
    /// `CreateProcessW` only ever appends `.exe`. `which` applies the full
    /// list, so a `.cmd` is reachable by a name that omits the extension.
    ///
    /// Uses an absolute path rather than mutating `PATH`, which is global
    /// state — `CreateProcess` skips the `.exe` append for any name containing
    /// a separator too, so this exercises the same gap. (#449)
    #[cfg(windows)]
    #[test]
    fn execute_runs_a_cmd_shim_named_without_its_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        let script = dir.path().join("tokf_probe.cmd");
        std::fs::write(&script, "@echo off\r\necho probe-ok\r\n").unwrap();

        let extensionless = dir.path().join("tokf_probe");
        let result = execute(&extensionless.to_string_lossy(), &[]).unwrap();

        assert_eq!(result.stdout.trim(), "probe-ok");
        assert_eq!(result.exit_code, 0);
    }

    /// A program whose *path* contains a space must reach the OS intact.
    /// This is the #450 repro: `C:\Program Files\...` was cut at the space and
    /// the leading half reported as the program.
    #[test]
    fn execute_runs_a_program_whose_path_contains_a_space() {
        let dir = tempfile::TempDir::new().unwrap();
        let spaced = dir.path().join("probe dir");
        std::fs::create_dir_all(&spaced).unwrap();

        #[cfg(windows)]
        let program = {
            let p = spaced.join("probe.cmd");
            std::fs::write(&p, "@echo off\r\necho probe-ok\r\n").unwrap();
            p
        };

        #[cfg(unix)]
        let program = {
            use std::os::unix::fs::PermissionsExt;
            let p = spaced.join("probe.sh");
            std::fs::write(&p, "#!/bin/sh\necho probe-ok\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };

        let result = execute(&program.to_string_lossy(), &[]).unwrap();
        assert_eq!(result.stdout.trim(), "probe-ok");
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
        let (program, args) = echo_program(&["hi"]);
        let result = execute_with_env(program, &args, &[]).unwrap();
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
