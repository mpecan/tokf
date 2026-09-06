//! Test-only helpers for driving the rewrite engine.
#![allow(clippy::expect_used)]

//!
//! Each helper builds its own [`Runtime`], so every test gets fresh
//! directories with no setup and no shared state to collide over.

use std::path::{Path, PathBuf};

use super::types::{RewriteConfig, RewriteOptions};
use super::{RewriteCtx, collect_filter_patterns, rewrite_with_config_and_options};
use crate::runtime::Runtime;

/// Run a rewrite with explicit config against an isolated runtime.
pub fn rewrite_isolated(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
) -> String {
    rewrite_isolated_with_options(
        command,
        user_config,
        search_dirs,
        verbose,
        &RewriteOptions::default(),
    )
}

pub fn rewrite_isolated_with_options(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    rewrite_with(
        &Runtime::isolated(),
        command,
        user_config,
        search_dirs,
        verbose,
        options,
    )
}

/// Run a rewrite with the runtime's working directory pointed at `cwd`, so
/// worktree detection has a real directory layout to read.
pub fn rewrite_in_cwd(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    cwd: &Path,
    options: &RewriteOptions,
) -> String {
    let rt = Runtime::builder().cwd(cwd).build();
    rewrite_with(&rt, command, user_config, search_dirs, false, options)
}

/// The one place tests construct a [`RewriteCtx`], so a new field on it is a
/// single edit rather than one per helper.
///
/// Over the argument limit because it is the union of what the public helpers
/// above take; collapsing it would just move the arguments to their call sites.
#[allow(clippy::too_many_arguments)]
fn rewrite_with(
    rt: &Runtime,
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
    verbose: bool,
    options: &RewriteOptions,
) -> String {
    rewrite_with_config_and_options(
        RewriteCtx {
            rt,
            user_config,
            search_dirs,
            no_cache: false,
        },
        command,
        verbose,
        options,
    )
}

pub fn collect_filter_patterns_isolated(search_dirs: &[PathBuf]) -> Vec<String> {
    let rt = Runtime::isolated();
    collect_filter_patterns(&rt, search_dirs, false)
}

/// Build a checkout whose `.git` file marks it a **linked** worktree, and
/// return its path.
pub fn linked_worktree(root: &Path) -> PathBuf {
    let wt = root.join("wt");
    std::fs::create_dir_all(&wt).expect("create worktree dir");
    std::fs::write(wt.join(".git"), "gitdir: /repo/.git/worktrees/agent-x\n")
        .expect("write .git file");
    wt
}

/// Build a checkout whose `.git` is a directory — an ordinary **main**
/// worktree — and return its path.
pub fn main_worktree(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("create .git dir");
    repo
}
