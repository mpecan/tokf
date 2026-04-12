//! Helpers for distinguishing real agent activity from test-fixture /
//! shell-prompt / temp-dir noise in the `tracking.db` event log.
//!
//! Background: when investigating mpecan/tokf#320 by hand, several
//! `git/status` "bursts" were inflated by:
//!
//! - statusline / shell-prompt callers (`/opt/homebrew/bin/git status`)
//! - `tokf verify` test fixtures running git inside `/var/folders/.../.tmpXXXX`
//! - Claude Code hooks calling `git status` before/after every tool call
//!
//! None of these are agent confusion, so the doctor command excludes them
//! by default. The user can disable the filter with `--include-noise`.

/// Path-substring patterns that mark a tracking event as "noise" — almost
/// certainly originating from a test fixture or temp-dir invocation, not
/// from an agent's normal workflow.
const NOISE_PATH_PATTERNS: &[&str] = &[
    "/var/folders/", // macOS temp dirs (used by tempfile / TempDir)
    "/tmp/",         // Linux temp dirs
    ".tokf-verify-", // tokf verify test rigs
    "/T/",           // common abbreviation in temp paths (shellcheck etc.)
    ".tmp",          // .tmpXXXX style names from rust tempfile
];

/// Returns true if `command` looks like a test-fixture / temp-dir
/// invocation that should be filtered out by default.
pub fn is_noise_command(command: &str) -> bool {
    NOISE_PATH_PATTERNS.iter().any(|p| command.contains(p))
}

/// Strips trailing path-shaped tokens from a command string.
///
/// Returns a "shape" that's safe to display without leaking sensitive
/// content like `curl -H 'Authorization: ...'`. Used as the default
/// human-output mode when `--show-commands` is off.
///
/// The shape preserves the program name and any leading short flags, then
/// substitutes `<args>` for the rest. This is intentionally crude: the
/// goal is "looks like the same command shape" not "perfect normalization".
pub fn command_shape(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut tokens = trimmed.split_whitespace();
    let Some(prog) = tokens.next() else {
        return String::new();
    };
    // Keep the program name and the first sub-command-looking token
    // (e.g. `git status`, `cargo test`, `npm run`). Anything after that
    // collapses to `<args>`. If there is no third token at all, omit the
    // `<args>` placeholder so `git status` stays `git status`.
    let Some(second) = tokens.next() else {
        return prog.to_string();
    };
    let has_more = tokens.next().is_some();
    if looks_like_subcommand(second) {
        if has_more {
            format!("{prog} {second} <args>")
        } else {
            format!("{prog} {second}")
        }
    } else {
        format!("{prog} <args>")
    }
}

/// A "subcommand" is a bare alphabetic word — no leading dash, no slash,
/// no equals sign. This catches `git status`, `cargo test`, `npm run`
/// while excluding `--name-only`, `/path/to/file`, `KEY=value`.
fn looks_like_subcommand(token: &str) -> bool {
    !token.is_empty()
        && !token.starts_with('-')
        && !token.contains('/')
        && !token.contains('=')
        && token.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_detects_var_folders() {
        assert!(is_noise_command("git -C /var/folders/abc/.tmpXYZ status"));
    }

    #[test]
    fn noise_detects_tokf_verify_rigs() {
        assert!(is_noise_command(
            "git -C /Users/x/.tokf-verify-cache/git_status status"
        ));
    }

    #[test]
    fn noise_detects_tmp_paths() {
        assert!(is_noise_command("ls /tmp/some-test"));
    }

    #[test]
    fn noise_does_not_flag_normal_commands() {
        assert!(!is_noise_command("git status"));
        assert!(!is_noise_command("git diff --stat"));
        assert!(!is_noise_command("cargo test"));
    }

    #[test]
    fn shape_strips_path_args() {
        assert_eq!(
            command_shape("git diff /home/user/repo/src/main.rs"),
            "git diff <args>"
        );
    }

    #[test]
    fn shape_keeps_subcommand() {
        assert_eq!(command_shape("git status"), "git status");
        assert_eq!(command_shape("cargo test"), "cargo test");
        assert_eq!(command_shape("npm run"), "npm run");
    }

    #[test]
    fn shape_with_flags_first() {
        // First token after program is a flag, not a subcommand → no
        // subcommand kept, just `<args>`.
        assert_eq!(command_shape("ls -la /tmp"), "ls <args>");
    }

    #[test]
    fn shape_lone_program() {
        assert_eq!(command_shape("ls"), "ls");
    }

    #[test]
    fn shape_empty_input() {
        assert_eq!(command_shape(""), "");
        assert_eq!(command_shape("   "), "");
    }

    #[test]
    fn shape_redacts_sensitive_args() {
        // The point of `command_shape` is that secrets in args don't leak.
        let shape =
            command_shape("curl -H Authorization:Bearer-secret-token https://api.example.com");
        assert!(!shape.contains("secret"));
        assert_eq!(shape, "curl <args>");
    }

    #[test]
    fn looks_like_subcommand_basic() {
        assert!(looks_like_subcommand("status"));
        assert!(looks_like_subcommand("test"));
        assert!(looks_like_subcommand("run"));
        assert!(looks_like_subcommand("name-only"));
        assert!(!looks_like_subcommand("--name-only"));
        assert!(!looks_like_subcommand("/path"));
        assert!(!looks_like_subcommand("KEY=val"));
        assert!(!looks_like_subcommand(""));
    }
}
