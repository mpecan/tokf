#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Tests for the "transparent-arg" command class — see issue #338.
//!
//! When a command's last argument is opaque shell code that runs in a
//! different environment (canonical example: `ssh HOST 'cmd'`), tokf must
//! not splice text into that argv via regex `[[rewrite]]` rules. The
//! built-in list (`ssh`, `mosh`, `slogin`) is always active; users can
//! extend via `[transparent] commands = […]`.

use std::fs;

use tempfile::TempDir;

use super::*;
use crate::rewrite::transparent::{
    BUILTIN_TRANSPARENT_COMMANDS, any_segment_is_transparent, is_transparent_command,
};

/// Build a config with one wildcard `[[rewrite]]` rule whose replacement
/// would mangle any inner argv. Used to assert that the rule is *not*
/// applied to transparent commands.
fn config_with_mangling_rule() -> RewriteConfig {
    RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^(.*)$".to_string(),
            replace: "mangled {0}".to_string(),
        }],
        permissions: None,
        debug: None,
        transparent: None,
    }
}

#[test]
fn ssh_with_quoted_arg_passthrough_when_no_filter() {
    // No filter for ssh — the standard `tokf run` wrap path doesn't fire,
    // and the user's wildcard rewrite rule must not apply either.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "ssh HOST 'ls -la /var/log'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "ssh HOST 'ls -la /var/log'");
}

#[test]
fn non_transparent_command_still_subject_to_user_rule() {
    // Sanity check: the wildcard rule *does* fire on non-transparent
    // commands. This pins the gating behaviour to the transparent class.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(result, "mangled git status");
}

#[test]
fn ssh_basename_match_with_full_path() {
    // /usr/bin/ssh must be treated identically to bare `ssh`.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "/usr/bin/ssh HOST cmd",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "/usr/bin/ssh HOST cmd");
}

#[test]
fn ssh_add_is_not_transparent() {
    // `ssh-add` is a sibling tool, not a remote-shell launcher — the
    // wildcard rule should still apply.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "ssh-add ~/.ssh/id_rsa",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "mangled ssh-add ~/.ssh/id_rsa");
}

#[test]
fn mosh_is_built_in_transparent() {
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "mosh HOST 'remote-cmd'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "mosh HOST 'remote-cmd'");
}

#[test]
fn slogin_is_built_in_transparent() {
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "slogin HOST cmd",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "slogin HOST cmd");
}

#[test]
fn user_can_extend_transparent_list() {
    // A user with `kubectl exec` workflows can add `kubectl` to the list.
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^(.*)$".to_string(),
            replace: "mangled {0}".to_string(),
        }],
        permissions: None,
        debug: None,
        transparent: Some(types::TransparentConfig {
            commands: vec!["kubectl".to_string()],
        }),
    };
    let result = rewrite_with_config(
        "kubectl exec POD -- cmd",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "kubectl exec POD -- cmd");
}

#[test]
fn user_extra_does_not_disable_built_ins() {
    // Adding kubectl to the user list must not silently turn off ssh.
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^(.*)$".to_string(),
            replace: "mangled {0}".to_string(),
        }],
        permissions: None,
        debug: None,
        transparent: Some(types::TransparentConfig {
            commands: vec!["kubectl".to_string()],
        }),
    };
    let result = rewrite_with_config("ssh HOST cmd", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(result, "ssh HOST cmd");
}

#[test]
fn ssh_filter_match_still_wraps_with_tokf_run() {
    // The argv-preserving `tokf run` wrap is still allowed for transparent
    // commands — it only prefixes, never splices into the argv. So if a
    // user has a filter for ssh, the outer wrap kicks in and the inner
    // quoted argument is left untouched.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ssh.toml"), "command = \"ssh\"").unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "ssh HOST 'docker ps'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run ssh HOST 'docker ps'");
}

#[test]
fn ssh_pipe_strip_preserves_inner_argv() {
    // Pipe stripping with `--baseline-pipe` is also argv-preserving — it
    // adds flags between `tokf run` and the command. Verify the inner
    // quoted argument is byte-for-byte preserved.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ssh.toml"), "command = \"ssh\"").unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "ssh HOST 'docker ps' | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        result,
        "tokf run --baseline-pipe 'head -5' ssh HOST 'docker ps'"
    );
}

#[test]
fn ssh_with_pipe_inside_quotes_unchanged() {
    // The `|` is inside the quoted ssh argument and runs on the remote.
    // rable's AST treats this as a single Command (no top-level pipe), so
    // pipe-strip logic must not fire. The inner argv must be byte-for-byte
    // preserved regardless of the user's `[pipe]` settings.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ssh.toml"), "command = \"ssh\"").unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "ssh HOST 'cmd | head -5'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // ssh has a filter, so the outer wrap fires — but the inner pipe is
    // sealed inside the quoted argument and must not be stripped or
    // injected as `--baseline-pipe`.
    assert_eq!(result, "tokf run ssh HOST 'cmd | head -5'");
}

#[test]
fn compound_with_ssh_segment_blocks_user_rule() {
    // The wildcard rule's regex matches the *whole* compound, so even when
    // ssh is in a later segment, applying the rule could splice text into
    // the ssh argv. Gate must trip via any_segment_is_transparent.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config(
        "cd /tmp && ssh HOST 'cmd'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // Whole compound is left untouched by the user rule; per-segment
    // wraps below still try to fire (cd has no filter, ssh has no filter
    // in this test, so the result is byte-for-byte identical to input).
    assert_eq!(result, "cd /tmp && ssh HOST 'cmd'");
}

#[test]
fn user_skip_pattern_takes_precedence_over_transparent_gate() {
    // `[skip]` runs first and is the documented escape hatch. A user who
    // has explicitly suppressed `ssh ` should still see no rewrite at all,
    // not even the argv-preserving filter wrap.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("ssh.toml"), "command = \"ssh\"").unwrap();

    let config = RewriteConfig {
        skip: Some(types::SkipConfig {
            patterns: vec!["^ssh ".to_string()],
        }),
        pipe: None,
        rewrite: vec![],
        permissions: None,
        debug: None,
        transparent: None,
    };
    let result = rewrite_with_config(
        "ssh HOST 'cmd'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "ssh HOST 'cmd'");
}

#[test]
fn ssh_basename_match_case_sensitive() {
    // Shell PATH lookup is case-sensitive on Linux, and even on macOS
    // (case-insensitive FS notwithstanding) we follow that convention.
    // `SSH HOST cmd` is a different command name and must NOT be
    // transparent — the user rule should mangle it.
    let dir = TempDir::new().unwrap();
    let config = config_with_mangling_rule();
    let result = rewrite_with_config("SSH HOST cmd", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(result, "mangled SSH HOST cmd");
}

// --- is_transparent_command unit tests ---

#[test]
fn is_transparent_command_built_ins() {
    assert!(is_transparent_command("ssh HOST cmd", &[]));
    assert!(is_transparent_command("mosh HOST cmd", &[]));
    assert!(is_transparent_command("slogin HOST cmd", &[]));
}

#[test]
fn is_transparent_command_basename() {
    assert!(is_transparent_command("/usr/bin/ssh HOST cmd", &[]));
    assert!(is_transparent_command("~/bin/ssh HOST cmd", &[]));
}

#[test]
fn is_transparent_command_env_prefix() {
    assert!(is_transparent_command(
        "SSH_AUTH_SOCK=/tmp/x ssh HOST cmd",
        &[]
    ));
}

#[test]
fn is_transparent_command_user_extras() {
    let extras = vec!["kubectl".to_string(), "doctl".to_string()];
    assert!(is_transparent_command("kubectl get pods", &extras));
    assert!(is_transparent_command("doctl k8s cluster list", &extras));
}

#[test]
fn is_transparent_command_negative() {
    assert!(!is_transparent_command("git status", &[]));
    assert!(!is_transparent_command("ssh-add", &[]));
    assert!(!is_transparent_command("scp file HOST:dst", &[]));
    assert!(!is_transparent_command("", &[]));
}

#[test]
fn builtin_list_contains_expected() {
    // Documents the built-in list at the test layer so an accidental
    // narrowing is caught in CI.
    assert!(BUILTIN_TRANSPARENT_COMMANDS.contains(&"ssh"));
    assert!(BUILTIN_TRANSPARENT_COMMANDS.contains(&"mosh"));
    assert!(BUILTIN_TRANSPARENT_COMMANDS.contains(&"slogin"));
}

// --- any_segment_is_transparent unit tests ---

#[test]
fn any_segment_is_transparent_single_segment() {
    assert!(any_segment_is_transparent("ssh HOST cmd", &[]));
    assert!(!any_segment_is_transparent("git status", &[]));
}

#[test]
fn any_segment_is_transparent_compound_first() {
    assert!(any_segment_is_transparent("ssh HOST cmd && echo done", &[]));
}

#[test]
fn any_segment_is_transparent_compound_middle() {
    assert!(any_segment_is_transparent(
        "cd /tmp && ssh HOST cmd && echo done",
        &[]
    ));
}

#[test]
fn any_segment_is_transparent_compound_last() {
    assert!(any_segment_is_transparent("cd /tmp && ssh HOST cmd", &[]));
}

#[test]
fn any_segment_is_transparent_no_match_anywhere() {
    assert!(!any_segment_is_transparent(
        "cd /tmp && ls -la && echo done",
        &[]
    ));
}

#[test]
fn any_segment_is_transparent_extras_apply_per_segment() {
    let extras = vec!["kubectl".to_string()];
    assert!(any_segment_is_transparent(
        "cd /tmp && kubectl get pods",
        &extras
    ));
    assert!(!any_segment_is_transparent(
        "cd /tmp && kubectl get pods",
        &[]
    ));
}
