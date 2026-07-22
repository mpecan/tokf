//! Verify-specific search-dir resolution.
//!
//! The reusable corpus walker lives in [`tokf::suite_discovery`]; this module
//! keeps only the parts that depend on verify's own CLI types.

use std::path::PathBuf;

use tokf::runtime::Runtime;

pub(super) use tokf::suite_discovery::{
    DiscoveredSuite, discover_all_filters_with_coverage, discover_suites,
};

// --- Search dirs for verify ---

// Intentionally different from `config::default_search_dirs()`: verify puts
// `filters/` (stdlib) first so repo developers test the stdlib by default,
// while the runtime puts `.tokf/filters/` (project overrides) first.
pub(super) fn verify_search_dirs(rt: &Runtime, scope: Option<&super::VerifyScope>) -> Vec<PathBuf> {
    match scope {
        Some(super::VerifyScope::Project) => {
            let mut dirs = Vec::new();
            if let Some(cwd) = rt.cwd() {
                dirs.push(cwd.join(".tokf/filters"));
            }
            dirs
        }
        Some(super::VerifyScope::Global) => {
            let mut dirs = Vec::new();
            if let Some(user) = rt.user_dir() {
                dirs.push(user.join("filters"));
            }
            dirs
        }
        Some(super::VerifyScope::Stdlib) => {
            let mut dirs = Vec::new();
            if let Some(cwd) = rt.cwd() {
                dirs.push(cwd.join("filters"));
            }
            dirs
        }
        None => {
            // Priority order (highest first):
            //   1. filters/ in CWD — catches the stdlib during repo development
            //   2. .tokf/filters/ in CWD — repo-local custom filters
            //   3. {config_dir}/tokf/filters/ — user-level custom filters
            // When the same filter name appears in multiple dirs, the first wins.
            let mut dirs = Vec::new();
            if let Some(cwd) = rt.cwd() {
                dirs.push(cwd.join("filters"));
                dirs.push(cwd.join(".tokf/filters"));
            }
            if let Some(user) = rt.user_dir() {
                dirs.push(user.join("filters"));
            }
            dirs
        }
    }
}
