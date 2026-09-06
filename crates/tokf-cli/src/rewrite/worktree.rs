//! The linked git worktree guard.
//!
//! An agent harness can isolate an agent to its own git worktree and enforce
//! that by statically parsing each command before running it. Rewriting
//! `git status` to `tokf run git status` puts an opaque wrapper in front of
//! the invocation the harness needs to read, so it refuses to run the command
//! at all. Leaving `git` alone inside a linked worktree keeps it legible.
//!
//! Detection reads git's own on-disk layout rather than matching a path
//! convention, so it holds for any worktree wherever it lives — a harness
//! directory like `.claude/worktrees/agent-x`, a sibling `../feature-y`, or
//! anything else `git worktree add` was pointed at.
//!
//! The rule git itself uses: in the **main** worktree `.git` is a directory;
//! in a **linked** worktree `.git` is a file holding `gitdir: <path>`, and
//! that path is `<common-dir>/worktrees/<id>`. Submodules also use a `.git`
//! file, but theirs points at `<common-dir>/modules/<name>` — so the parent
//! component of the target is what separates the two cases.

use std::cell::OnceCell;
use std::path::Path;

use super::bash_ast::first_command_basename;
use super::types::{RewriteConfig, RewriteOptions};
use crate::runtime::Runtime;

/// Decides whether a command should be left unrewritten to stay legible to a
/// harness that re-reads it.
///
/// The filesystem walk is deferred and memoised: [`Self::claims`] tests the
/// command's shape first, so the overwhelming majority of commands an agent
/// runs — none of which are `git` — never touch the disk. That matters
/// because the guard sits on the hook path, which runs on every command.
pub struct GitGuard<'a> {
    /// Directory to inspect, or `None` when the guard cannot apply at all:
    /// the caller opted out, the user disabled it, or the cwd is unknown.
    /// Storing the disabled case as `None` keeps the check to one branch.
    cwd: Option<&'a Path>,
    verbose: bool,
    /// Memoised ancestor walk, forced by the first `git` command seen.
    inside: OnceCell<bool>,
}

impl<'a> GitGuard<'a> {
    /// Build a guard for this rewrite.
    ///
    /// Both opt-outs are folded in here — `options` for callers whose output
    /// nothing re-parses, and the user's `[worktree] skip_git` — so that a
    /// disabled guard costs one `Option` test per command and no I/O.
    pub fn new(
        rt: &'a Runtime,
        user_config: &RewriteConfig,
        options: &RewriteOptions,
        verbose: bool,
    ) -> Self {
        let enabled =
            options.guard_worktree_git && user_config.worktree.as_ref().is_none_or(|w| w.skip_git);
        Self {
            cwd: enabled.then(|| rt.cwd()).flatten(),
            verbose,
            inside: OnceCell::new(),
        }
    }

    /// True when `command` is a `git` invocation inside a linked worktree.
    ///
    /// Evaluated per segment rather than per command line: in
    /// `git add . && cargo test` only the git half needs to stay legible, and
    /// the cargo half should keep its filter.
    pub fn claims(&self, command: &str) -> bool {
        is_git_invocation(command) && self.inside_linked_worktree()
    }

    /// True when **any** segment of a compound command is claimed.
    ///
    /// Mirrors [`super::transparent::any_segment_is_transparent`], and exists
    /// for the same reason: user `[[rewrite]]` rules match against the whole
    /// command string, so one guarded segment anywhere — even behind a
    /// `cd … &&` — is enough to make splicing a wrapper into it unsafe.
    pub fn claims_any_segment(&self, segments: &[(String, String)]) -> bool {
        segments.iter().any(|(seg, _)| self.claims(seg.trim()))
    }

    /// The memoised filesystem walk, forced at most once per rewrite.
    fn inside_linked_worktree(&self) -> bool {
        let Some(cwd) = self.cwd else {
            return false;
        };
        *self.inside.get_or_init(|| {
            let inside = in_linked_worktree(cwd);
            if inside && self.verbose {
                eprintln!(
                    "[tokf] worktree: linked git worktree detected, leaving git commands \
                     unrewritten (set [worktree] skip_git = false in rewrites.toml to filter \
                     them anyway)"
                );
            }
            inside
        })
    }
}

/// True when the first word of `command` invokes `git`.
///
/// Shares [`first_command_basename`] with the transparent-command check, so
/// quoted (`'git' status`), path-qualified (`/usr/bin/git status`) and
/// env-prefixed (`GIT_PAGER=cat git log`) forms are all recognised the same
/// way here as everywhere else in the rewrite engine.
fn is_git_invocation(command: &str) -> bool {
    first_command_basename(command).is_some_and(|name| name == "git")
}

/// True when `dir` — or the nearest ancestor holding a `.git` entry — is a
/// linked git worktree.
///
/// Returns false for the main worktree, for submodules, and for a path that is
/// not in a git repository at all. Anything unreadable or malformed is also
/// false: a wrong "yes" silently costs the user filtering on every `git`
/// command, so ambiguity resolves toward normal behaviour.
fn in_linked_worktree(dir: &Path) -> bool {
    // `ancestors()` stops at the filesystem root on its own; a repository
    // boundary is whichever ancestor carries `.git` first.
    for ancestor in dir.ancestors() {
        let dot_git = ancestor.join(".git");
        let Ok(meta) = std::fs::symlink_metadata(&dot_git) else {
            continue;
        };
        // Whatever `.git` is here settles the question, so stop rather than
        // walking on into an enclosing repository: a directory is the main
        // worktree, and a symlink or anything else exotic we won't guess at.
        return meta.is_file()
            && std::fs::read_to_string(&dot_git)
                .is_ok_and(|contents| gitdir_points_into_worktrees(&contents));
    }
    false
}

/// True when a `.git` file's `gitdir:` line has the
/// `<common-dir>/worktrees/<id>` shape.
///
/// Only the first line is read — that is all git writes — and the value is
/// trimmed, since git terminates the line with a newline. An empty value
/// needs no separate guard: `Path::new("").parent()` is `None`.
///
/// Checking the parent component rather than searching for `worktrees`
/// anywhere in the path keeps a repository that merely *lives* under a
/// directory called `worktrees` from being misread as a linked worktree. The
/// path is compared as written: `git worktree add --relative-paths` records
/// `../../.git/worktrees/<id>`, which has the same shape.
fn gitdir_points_into_worktrees(contents: &str) -> bool {
    let Some(value) = contents
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("gitdir:"))
    else {
        return false;
    };
    Path::new(value.trim())
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == "worktrees")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::rewrite::test_helpers::{linked_worktree, main_worktree};

    #[test]
    fn main_worktree_is_not_linked() {
        let dir = TempDir::new().unwrap();
        assert!(!in_linked_worktree(&main_worktree(dir.path())));
    }

    #[test]
    fn linked_worktree_is_detected() {
        let dir = TempDir::new().unwrap();
        assert!(in_linked_worktree(&linked_worktree(dir.path())));
    }

    #[test]
    fn linked_worktree_detected_from_nested_subdirectory() {
        let dir = TempDir::new().unwrap();
        let wt = linked_worktree(dir.path());
        let nested = wt.join("crates").join("thing").join("src");
        fs::create_dir_all(&nested).unwrap();
        assert!(in_linked_worktree(&nested));
    }

    #[test]
    fn main_worktree_shadows_an_enclosing_linked_worktree() {
        // The first `.git` found wins, so a plain repo checked out inside a
        // worktree is judged on its own layout.
        let dir = TempDir::new().unwrap();
        let wt = linked_worktree(dir.path());
        assert!(!in_linked_worktree(&main_worktree(&wt)));
    }

    #[test]
    fn outside_a_repository_is_not_linked() {
        let dir = TempDir::new().unwrap();
        assert!(!in_linked_worktree(dir.path()));
    }

    #[test]
    fn unreadable_git_file_is_not_linked() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".git"), "not a gitdir line\n").unwrap();
        assert!(!in_linked_worktree(dir.path()));
    }

    #[test]
    fn gitdir_shapes() {
        // Linked worktrees, including the `--relative-paths` form (git 2.48+)
        // and names containing spaces.
        assert!(gitdir_points_into_worktrees(
            "gitdir: /repo/.git/worktrees/x\n"
        ));
        assert!(gitdir_points_into_worktrees(
            "gitdir: ../../.git/worktrees/agent-x\n"
        ));
        assert!(gitdir_points_into_worktrees(
            "gitdir: /repo/.git/worktrees/my branch\n"
        ));
        // Submodule.
        assert!(!gitdir_points_into_worktrees(
            "gitdir: ../.git/modules/sub\n"
        ));
        // A repository that merely lives under a directory named `worktrees`.
        assert!(!gitdir_points_into_worktrees(
            "gitdir: /home/me/worktrees/project/.git\n"
        ));
        // Malformed or absent values.
        assert!(!gitdir_points_into_worktrees("gitdir:   \n"));
        assert!(!gitdir_points_into_worktrees("not a gitdir line\n"));
        assert!(!gitdir_points_into_worktrees("ref: refs/heads/main\n"));
        assert!(!gitdir_points_into_worktrees(""));
    }

    #[test]
    fn git_invocation_shapes() {
        assert!(is_git_invocation("git status"));
        assert!(is_git_invocation("/usr/bin/git status"));
        assert!(is_git_invocation("GIT_PAGER=cat git log"));
        // Shared with the transparent-command check, so quoting is handled.
        assert!(is_git_invocation("'git' status"));
        assert!(!is_git_invocation("gitleaks detect"));
        assert!(!is_git_invocation("legit status"));
        assert!(!is_git_invocation(""));
    }
}
