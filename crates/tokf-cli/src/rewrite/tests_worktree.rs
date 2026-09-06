//! Integration tests for the linked-worktree git guard.
//!
//! Inside a linked git worktree, `git` commands are left unrewritten by
//! default so that an agent harness enforcing worktree isolation can still
//! recognise them. Everything else keeps filtering normally.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;
use crate::rewrite::test_helpers::{linked_worktree, main_worktree};

/// Options as the hook sets them — the caller the guard exists for.
fn hook_options() -> RewriteOptions {
    RewriteOptions {
        no_mask_exit_code: false,
        guard_worktree_git: true,
    }
}

/// A filter directory covering the commands these tests exercise.
fn filters() -> (TempDir, Vec<PathBuf>) {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();
    fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();
    let paths = vec![dir.path().to_path_buf()];
    (dir, paths)
}

fn opt_out() -> RewriteConfig {
    RewriteConfig {
        worktree: Some(types::WorktreeConfig { skip_git: false }),
        ..Default::default()
    }
}

#[test]
fn git_is_not_rewritten_inside_a_linked_worktree() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "git status",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "git status");
}

#[test]
fn git_is_rewritten_in_a_main_worktree() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let repo = main_worktree(home.path());

    let r = rewrite_in_cwd(
        "git status",
        &RewriteConfig::default(),
        &dirs,
        &repo,
        &hook_options(),
    );
    assert_eq!(r, "tokf run git status");
}

#[test]
fn non_git_commands_still_filter_inside_a_linked_worktree() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "cargo test",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "tokf run cargo test");
}

#[test]
fn opting_out_restores_git_rewriting_inside_a_linked_worktree() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd("git status", &opt_out(), &dirs, &wt, &hook_options());
    assert_eq!(r, "tokf run git status");
}

#[test]
fn guard_applies_per_segment_in_a_compound_command() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "git add . && cargo test",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "git add . && tokf run cargo test");
}

#[test]
fn guard_covers_piped_git_commands() {
    // The piped path rewrites to `tokf run --baseline-pipe …`, which hides the
    // git invocation just as thoroughly as a plain wrap.
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "git diff | head -50",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "git diff | head -50");
}

#[test]
fn guard_covers_an_env_prefixed_git_command() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "GIT_PAGER=cat git status",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "GIT_PAGER=cat git status");
}

#[test]
fn guard_covers_git_invoked_by_absolute_path() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "/usr/bin/git status",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "/usr/bin/git status");
}

#[test]
fn guard_does_not_catch_commands_merely_containing_git() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());
    fs::write(
        dirs[0].join("gitleaks.toml"),
        "command = \"gitleaks detect\"",
    )
    .unwrap();

    let r = rewrite_in_cwd(
        "gitleaks detect",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "tokf run gitleaks detect");
}

#[test]
fn user_rewrite_rules_for_git_are_also_suppressed_in_a_linked_worktree() {
    // A user `[[rewrite]]` rule produces the same opaque wrapper, so the guard
    // has to win over it — otherwise the escape hatch leaks the problem back.
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());
    let config = RewriteConfig {
        rewrite: vec![types::RewriteRule {
            match_pattern: "^git status".to_string(),
            replace: "tokf summary {0}".to_string(),
        }],
        ..Default::default()
    };

    let r = rewrite_in_cwd("git status", &config, &dirs, &wt, &hook_options());
    assert_eq!(r, "git status");
}

#[test]
fn no_guard_outside_a_repository() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let plain = home.path().join("plain");
    fs::create_dir_all(&plain).unwrap();

    let r = rewrite_in_cwd(
        "git status",
        &RewriteConfig::default(),
        &dirs,
        &plain,
        &hook_options(),
    );
    assert_eq!(r, "tokf run git status");
}

#[test]
fn no_guard_in_a_submodule() {
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let sub = home.path().join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();

    let r = rewrite_in_cwd(
        "git status",
        &RewriteConfig::default(),
        &dirs,
        &sub,
        &hook_options(),
    );
    assert_eq!(r, "tokf run git status");
}

#[test]
fn shell_mode_keeps_git_filtering_inside_a_linked_worktree() {
    // `tokf -c` (make/just, PATH shims) hands its result to `sh`. Nothing
    // re-reads it, so there is no reason to give up the filter there.
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let shell = RewriteOptions {
        no_mask_exit_code: true,
        guard_worktree_git: false,
    };
    let r = rewrite_in_cwd("git status", &RewriteConfig::default(), &dirs, &wt, &shell);
    assert_eq!(r, "tokf run --no-mask-exit-code git status");
}

#[test]
fn user_rewrite_rules_are_suppressed_for_a_guarded_compound_segment() {
    // The rule matches the whole command string, so firing it would splice a
    // wrapper back in front of the git half the guard has to keep legible.
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());
    let config = RewriteConfig {
        rewrite: vec![types::RewriteRule {
            match_pattern: "^git add".to_string(),
            replace: "tokf summary {0}".to_string(),
        }],
        ..Default::default()
    };

    let r = rewrite_in_cwd(
        "git add . && cargo test",
        &config,
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "git add . && tokf run cargo test");
}

#[test]
fn quoted_git_is_guarded() {
    // Shared `first_command_basename` handles quoting; the hand-rolled
    // word-split it replaced did not.
    let (_f, dirs) = filters();
    let home = TempDir::new().unwrap();
    let wt = linked_worktree(home.path());

    let r = rewrite_in_cwd(
        "'git' status",
        &RewriteConfig::default(),
        &dirs,
        &wt,
        &hook_options(),
    );
    assert_eq!(r, "'git' status");
}
