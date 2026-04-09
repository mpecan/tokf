//! Configuration for the `[tree]` transform.
//!
//! When a filter emits a list of file paths, common directory prefixes are
//! repeated on every line. The `[tree]` section restructures the output into
//! a directory tree, writing each shared prefix once. See
//! `crates/tokf-filter/src/filter/tree.rs` for the algorithm and
//! `docs/writing-filters.md` for end-user documentation.

use serde::{Deserialize, Serialize};

/// Per-filter configuration for the `[tree]` transform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeConfig {
    /// Regex with two capture groups: (1) the leaf decoration to keep
    /// (e.g. `"M  "`, `"?? "`, or `""`), (2) the path itself.
    pub pattern: String,

    /// When `true`, lines that don't match `pattern` are kept verbatim
    /// at their original position (e.g. branch headers in `git status`).
    /// When `false`, unmatched lines are dropped.
    #[serde(default = "default_true")]
    pub passthrough_unmatched: bool,

    /// Minimum number of matched lines for the tree to engage. Below this,
    /// the tree adds more overhead than it saves and the original flat
    /// output is returned unchanged.
    #[serde(default = "default_min_files")]
    pub min_files: usize,

    /// Minimum number of leading path components shared across all matched
    /// lines for the tree to engage. `0` engages even with no shared root
    /// (rare); `1` requires at least one common directory level.
    #[serde(default = "default_min_shared_depth")]
    pub min_shared_depth: usize,

    /// Visual rendering style for tree connectors.
    #[serde(default)]
    pub style: TreeStyle,

    /// Collapse single-child internal directories into their parent
    /// (e.g. `src/lib/foo.rs` + `src/lib/bar.rs` rendered under a single
    /// `src/lib/` node instead of nested `src/` → `lib/`).
    #[serde(default = "default_true")]
    pub collapse_single_child: bool,

    /// Sort children alphabetically. Off by default — source order is
    /// stable and predictable for LLMs.
    #[serde(default)]
    pub sort: bool,
}

/// Visual style for tree connectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TreeStyle {
    /// Unicode box-drawing: `├─ │  └─`
    #[default]
    Unicode,
    /// ASCII fallback: ``|- |  `-``
    Ascii,
    /// Plain two-space indent per level, no connectors.
    Indent,
}

const fn default_true() -> bool {
    true
}

const fn default_min_files() -> usize {
    4
}

const fn default_min_shared_depth() -> usize {
    1
}
