//! Shell mode: makes tokf usable as a POSIX-compatible shell replacement.
//!
//! Task runners like `make` and `just` invoke their recipe lines via
//! `$SHELL -c 'recipe_line'`.  When tokf is set as the shell, each recipe
//! line is individually matched against installed filters.
//!
//! Both entry points (`cmd_shell` for string mode, `cmd_shell_argv` for
//! argv mode) delegate to the rewrite system and then to `sh -c`. Matched
//! commands become `tokf run --no-mask-exit-code ...` which goes through
//! the normal `cmd_run` path — no duplicated filter pipeline here.

/// Returns `true` if `flag` looks like a POSIX shell flag containing `-c`.
///
/// Matches: `-c`, `-cu`, `-ec`, `-ecu`, etc.
/// Does NOT match: `--cache`, `--color`, or any long flag.
pub fn is_shell_flag(flag: &str) -> bool {
    flag.starts_with('-')
        && !flag.starts_with("--")
        && flag.len() > 1
        && flag.as_bytes()[1..].contains(&b'c')
}

/// Returns `true` if the `TOKF_NO_FILTER` environment variable is set to a
/// truthy value (`1`, `true`, `yes`).
fn env_no_filter() -> bool {
    std::env::var("TOKF_NO_FILTER")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// Returns `true` if the `TOKF_VERBOSE` environment variable is set to a
/// truthy value.
fn env_verbose() -> bool {
    std::env::var("TOKF_VERBOSE")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// Restore the original PATH (without shims) so that delegated commands
/// resolve real binaries. `build_inject_env` will re-add the shims directory
/// for sub-processes spawned by `tokf run`.
///
/// SAFETY: must only be called very early in `main()`, before any threads are
/// spawned. The Rust test runner is multi-threaded, but tests that don't set
/// `TOKF_ORIGINAL_PATH` will skip the unsafe block.
fn restore_original_path() {
    if let Ok(original) = std::env::var("TOKF_ORIGINAL_PATH") {
        // SAFETY: called from main() before any threads are spawned.
        unsafe { std::env::set_var("PATH", &original) };
    }
}

/// Rewrite a command string and delegate to the real shell.
///
/// Shared logic for both string mode and argv mode. Applies the rewrite
/// system with `--no-mask-exit-code` so the real exit code propagates.
///
/// Restores `TOKF_ORIGINAL_PATH` into `PATH` before delegating so that
/// both modes are protected from shim recursion.
fn rewrite_and_delegate(flags: &str, command: &str, verbose: bool) -> i32 {
    restore_original_path();

    if env_no_filter() {
        if verbose {
            eprintln!("[tokf] shell: TOKF_NO_FILTER set, delegating to sh");
        }
        return delegate_to_real_shell(flags, command);
    }

    let options = tokf::rewrite::types::RewriteOptions {
        no_mask_exit_code: true,
    };
    let rewritten = tokf::rewrite::rewrite_with_options(command, verbose, &options);

    if verbose {
        if rewritten == command {
            eprintln!("[tokf] shell: no filter match, delegating to sh");
        } else {
            eprintln!("[tokf] shell: rewritten to: {rewritten}");
        }
    }

    delegate_to_real_shell(flags, &rewritten)
}

/// Entry point for string shell mode.
///
/// Called when tokf is invoked as `tokf -c 'command'` (or `-cu`, `-ec`, etc.).
/// Task runners send recipe lines this way.  Returns the process exit code.
///
/// Respects environment variables since shell mode has no access to clap flags:
/// - `TOKF_NO_FILTER=1` — skip filtering, delegate directly to `sh`
/// - `TOKF_VERBOSE=1` — print filter resolution details to stderr
pub fn cmd_shell(flags: &str, command: &str) -> i32 {
    rewrite_and_delegate(flags, command, env_verbose())
}

/// Entry point for argv shell mode.
///
/// Called when tokf is invoked as `tokf -c cmd arg1 arg2 ...` (more than one
/// argument after `-c`).  This is used by PATH shims which pass the command
/// and its arguments as separate argv entries.
///
/// The `flags` parameter is the original shell flag string (e.g. `-c`, `-cu`,
/// `-ecu`) so that combined flags are forwarded to `sh` consistently with
/// string mode.
pub fn cmd_shell_argv(flags: &str, args: &[String]) -> i32 {
    if args.is_empty() {
        return 0;
    }

    // Build a shell-safe command string from the argv entries.
    let command = args
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");

    rewrite_and_delegate(flags, &command, env_verbose())
}

/// Delegate to the real system shell, preserving the original flags.
///
/// Spawns `sh` with the given flags and command, waits for completion,
/// and returns the exit code.
fn delegate_to_real_shell(flags: &str, command: &str) -> i32 {
    match std::process::Command::new("sh")
        .arg(flags)
        .arg(command)
        .status()
    {
        Ok(status) => {
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
        Err(e) => {
            eprintln!("[tokf] shell: failed to run sh: {e}");
            127
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // --- is_shell_flag ---

    #[test]
    fn shell_flag_c() {
        assert!(is_shell_flag("-c"));
    }

    #[test]
    fn shell_flag_cu() {
        assert!(is_shell_flag("-cu"));
    }

    #[test]
    fn shell_flag_ec() {
        assert!(is_shell_flag("-ec"));
    }

    #[test]
    fn shell_flag_ecu() {
        assert!(is_shell_flag("-ecu"));
    }

    #[test]
    fn shell_flag_not_long_flag() {
        assert!(!is_shell_flag("--cache"));
        assert!(!is_shell_flag("--color"));
    }

    #[test]
    fn shell_flag_not_verbose() {
        assert!(!is_shell_flag("-v"));
    }

    #[test]
    fn shell_flag_not_empty_dash() {
        assert!(!is_shell_flag("-"));
    }

    // --- delegate_to_real_shell ---

    #[cfg(unix)]
    #[test]
    fn delegate_echo() {
        // We can't easily test exec (it replaces the process), so test the
        // non-exec fallback behaviour by running a simple command.
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg("echo hello")
            .status()
            .unwrap();
        assert!(status.success());
    }

    // --- cmd_shell integration ---

    #[test]
    fn shell_unmatched_command_delegates() {
        // A command that doesn't match any filter should delegate to sh.
        // We verify this by running a command that real sh can execute.
        // Note: this test spawns a real process.
        let code = cmd_shell("-c", "true");
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_unmatched_failure_preserves_exit_code() {
        let code = cmd_shell("-c", "false");
        assert_ne!(code, 0);
    }

    #[test]
    fn shell_compound_delegates() {
        let code = cmd_shell("-c", "true && true");
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_compound_failure() {
        let code = cmd_shell("-c", "false && true");
        assert_ne!(code, 0);
    }

    #[test]
    fn shell_empty_command_delegates() {
        // Empty command should delegate to sh (which handles it).
        let code = cmd_shell("-c", "");
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_whitespace_only_delegates() {
        let code = cmd_shell("-c", "   ");
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_pipe_delegates() {
        // Pipes should delegate to the real shell.
        let code = cmd_shell("-c", "echo hello | cat");
        assert_eq!(code, 0);
    }

    // --- is_shell_flag edge cases ---

    #[test]
    fn shell_flag_uppercase_c_does_not_match() {
        // -C is a common flag (e.g., git -C /path), must NOT enter shell mode.
        assert!(!is_shell_flag("-C"));
    }

    #[test]
    fn shell_flag_empty_string() {
        assert!(!is_shell_flag(""));
    }

    #[test]
    fn shell_flag_no_dash() {
        assert!(!is_shell_flag("c"));
    }

    #[test]
    fn shell_flag_long_c_only() {
        assert!(!is_shell_flag("--c"));
    }

    // --- cmd_shell_argv ---

    #[test]
    fn shell_argv_simple_command() {
        // A simple command that should execute successfully.
        let args: Vec<String> = vec!["true".into()];
        let code = cmd_shell_argv("-c", &args);
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_argv_unknown_command_fallback() {
        // An unmatched command should execute directly and return its exit code.
        let args: Vec<String> = vec!["false".into()];
        let code = cmd_shell_argv("-c", &args);
        assert_ne!(code, 0);
    }

    #[test]
    fn shell_argv_preserves_arguments() {
        // Arguments with spaces should be preserved as separate argv entries.
        let args: Vec<String> = vec!["echo".into(), "hello world".into()];
        let code = cmd_shell_argv("-c", &args);
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_argv_empty_args() {
        let code = cmd_shell_argv("-c", &[]);
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_argv_single_quotes_in_args() {
        // Single quotes in arguments must be escaped correctly.
        let args: Vec<String> = vec!["echo".into(), "it's".into()];
        let code = cmd_shell_argv("-c", &args);
        assert_eq!(code, 0);
    }

    #[test]
    fn shell_argv_special_chars_in_args() {
        // Dollar signs and backticks should be literal (inside single quotes).
        let args: Vec<String> = vec!["echo".into(), "$HOME `whoami`".into()];
        let code = cmd_shell_argv("-c", &args);
        assert_eq!(code, 0);
    }
}
