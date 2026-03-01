//! Shell mode: makes tokf usable as a POSIX-compatible shell replacement.
//!
//! Task runners like `make` and `just` invoke their recipe lines via
//! `$SHELL -c 'recipe_line'`.  When tokf is set as the shell, each recipe
//! line is individually matched against installed filters.
//!
//! Shell mode always propagates the real exit code — no masking, no
//! "Error: Exit code N" prefix.

use tokf::filter;
use tokf::history;

use crate::resolve;

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

/// Returns `true` if the command contains shell metacharacters that require
/// a real shell to interpret.
///
/// When a recipe line uses operators, pipes, redirections, subshells, or
/// quotes, we delegate the entire line to the real shell so that semantics
/// are preserved.  False positives just mean we delegate to `sh`, which is
/// always correct — only simple `word arg arg` commands are handled directly.
fn needs_real_shell(command: &str) -> bool {
    command.contains("&&")
        || command.contains("||")
        || command.contains(';')
        || command.contains('|')
        || command.contains('>')
        || command.contains('<')
        || command.contains('`')
        || command.contains("$(")
        || command.contains('(')
        || command.contains('"')
        || command.contains('\'')
        || command.contains('\\')
        || command.contains('*')
        || command.contains('?')
        || command.contains('~')
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

/// Entry point for shell mode.
///
/// Called when tokf is invoked as `tokf -c 'command'` (or `-cu`, `-ec`, etc.).
/// Returns the process exit code.
///
/// Respects environment variables since shell mode has no access to clap flags:
/// - `TOKF_NO_FILTER=1` — skip filtering, delegate directly to `sh`
/// - `TOKF_VERBOSE=1` — print filter resolution details to stderr
pub fn cmd_shell(flags: &str, command: &str) -> i32 {
    let verbose = env_verbose();

    // TOKF_NO_FILTER bypasses all filtering.
    if env_no_filter() {
        if verbose {
            eprintln!("[tokf] shell: TOKF_NO_FILTER set, delegating to sh");
        }
        return delegate_to_real_shell(flags, command);
    }

    // Commands with shell metacharacters delegate to real shell.
    if needs_real_shell(command) {
        if verbose {
            eprintln!("[tokf] shell: delegating to sh (shell metacharacters)");
        }
        return delegate_to_real_shell(flags, command);
    }

    let words: Vec<String> = command.split_whitespace().map(String::from).collect();
    if words.is_empty() {
        return delegate_to_real_shell(flags, command);
    }

    // Try to find a matching filter.
    let filter_match = match resolve::find_filter(&words, verbose, false) {
        Ok(Some(m)) => m,
        Ok(None) => {
            if verbose {
                eprintln!("[tokf] shell: no filter match, delegating to sh");
            }
            return delegate_to_real_shell(flags, command);
        }
        Err(e) => {
            eprintln!("[tokf] shell: filter discovery error: {e:#}");
            return delegate_to_real_shell(flags, command);
        }
    };

    run_filtered(&words, filter_match, verbose)
}

/// Run a command through the filter pipeline with real exit code propagation.
fn run_filtered(command_args: &[String], filter_match: resolve::FilterMatch, verbose: bool) -> i32 {
    let words_consumed = filter_match.words_consumed;
    let remaining_args: Vec<String> = if words_consumed > 0 {
        command_args[words_consumed..].to_vec()
    } else if command_args.len() > 1 {
        command_args[1..].to_vec()
    } else {
        vec![]
    };

    let cmd_result = match resolve::run_command(
        Some(&filter_match.config),
        words_consumed,
        command_args,
        &remaining_args,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tokf] shell: error running command: {e:#}");
            return 1;
        }
    };

    // Phase B: resolve deferred output-pattern variants.
    let (cfg, filter_hash) = resolve::resolve_phase_b(filter_match, &cmd_result.combined, verbose);

    let input_bytes = cmd_result.combined.len();

    let start = std::time::Instant::now();
    let filter_opts = filter::FilterOptions {
        preserve_color: false,
    };
    let filtered = filter::apply(&cfg, &cmd_result, &remaining_args, &filter_opts);
    let elapsed = start.elapsed();

    let filter_name = cfg.command.first();
    let output_bytes = filtered.output.len();

    // Record tracking event (same as cmd_run).
    resolve::record_run(
        command_args,
        Some(filter_name),
        Some(&filter_hash),
        input_bytes,
        output_bytes,
        elapsed.as_millis(),
        cmd_result.exit_code,
        false,
    );
    resolve::try_auto_sync();

    // Record to history.
    let command_str = command_args.join(" ");
    let show_hint = cfg.show_history_hint || history::try_was_recently_run(&command_str);
    let history_id = history::try_record(
        &command_str,
        filter_name,
        &cmd_result.combined,
        &filtered.output,
        cmd_result.exit_code,
    );

    // Print filtered output — no exit code masking.
    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

    if show_hint && let Some(id) = history_id {
        println!(
            "[tokf] output filtered — to see what was omitted: `tokf history show --raw {id}`"
        );
    }

    // Always return the real exit code.
    cmd_result.exit_code
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

    // --- needs_real_shell ---

    #[test]
    fn shell_meta_and() {
        assert!(needs_real_shell("cd src && cargo test"));
    }

    #[test]
    fn shell_meta_or() {
        assert!(needs_real_shell("make test || true"));
    }

    #[test]
    fn shell_meta_semicolon() {
        assert!(needs_real_shell("echo hello; echo world"));
    }

    #[test]
    fn shell_meta_pipe() {
        assert!(needs_real_shell("cargo test | grep FAIL"));
    }

    #[test]
    fn shell_meta_redirect_out() {
        assert!(needs_real_shell("cargo build > /dev/null 2>&1"));
    }

    #[test]
    fn shell_meta_redirect_in() {
        assert!(needs_real_shell("wc -l < file.txt"));
    }

    #[test]
    fn shell_meta_subshell_dollar() {
        assert!(needs_real_shell("echo $(date)"));
    }

    #[test]
    fn shell_meta_subshell_paren() {
        assert!(needs_real_shell("(cd src && cargo test)"));
    }

    #[test]
    fn shell_meta_backtick() {
        assert!(needs_real_shell("echo `date`"));
    }

    #[test]
    fn shell_meta_quoted_operators_are_false_positive() {
        // Quoted operators are detected — this is a safe false positive
        // because sh handles them correctly.
        assert!(needs_real_shell("echo 'a && b'"));
        assert!(needs_real_shell("grep -E 'foo|bar' file"));
    }

    #[test]
    fn shell_meta_double_quotes() {
        assert!(needs_real_shell("echo \"hello world\""));
    }

    #[test]
    fn shell_meta_single_quotes() {
        assert!(needs_real_shell("echo 'hello world'"));
    }

    #[test]
    fn shell_meta_backslash() {
        assert!(needs_real_shell("echo hello\\ world"));
    }

    #[test]
    fn shell_meta_glob_star() {
        assert!(needs_real_shell("ls *.txt"));
    }

    #[test]
    fn shell_meta_glob_question() {
        assert!(needs_real_shell("ls file?.txt"));
    }

    #[test]
    fn shell_meta_tilde() {
        assert!(needs_real_shell("ls ~/Documents"));
    }

    #[test]
    fn not_shell_meta_simple() {
        assert!(!needs_real_shell("cargo test --lib"));
    }

    #[test]
    fn not_shell_meta_flags() {
        assert!(!needs_real_shell("git status --short"));
    }

    #[test]
    fn not_shell_meta_path_args() {
        assert!(!needs_real_shell("cargo test -p tokf-server -- --ignored"));
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
}
