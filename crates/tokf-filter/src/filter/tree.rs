//! `[tree]` transform — restructures a list of decorated paths into a
//! directory tree, writing each shared prefix once.
//!
//! Pipeline slot: runs after `dedup` and before the `pre_filtered` join in
//! `crate::filter::apply_internal`. When active, color restoration is
//! bypassed (color spans don't survive structural rearrangement).
//!
//! Algorithm:
//!   1. Compile `cfg.pattern`. Each input line either matches (yielding a
//!      decoration + path) or is unmatched and (optionally) preserved.
//!   2. **Engagement gates** — if too few matched (`cfg.min_files`) or the
//!      shared root depth is below `cfg.min_shared_depth`, return the
//!      original lines unchanged. Note: with `min_shared_depth = 0`,
//!      divergent multi-root inputs still render as a tree; set
//!      `min_shared_depth >= 1` to require a common root.
//!   3. Build a trie keyed on path components.
//!   4. Optionally collapse single-child internal directories
//!      (`src/lib/foo.rs` instead of nested `src/` → `lib/`).
//!   5. Render with the chosen `TreeStyle`.
//!
//! See `tokf_common::config::tree::TreeConfig` for configuration.

use std::sync::{Mutex, OnceLock};

use regex::Regex;

use tokf_common::config::tree::{TreeConfig, TreeStyle};

/// Apply the tree transform to a list of lines.
///
/// Returns `None` when the transform falls back to flat output (engagement
/// gates failed, regex didn't compile, etc.). Callers should treat `None`
/// as "use the original lines unchanged".
///
/// Returns `Some(rendered)` when the transform engaged.
pub(super) fn apply_tree(cfg: &TreeConfig, lines: &[&str]) -> Option<Vec<String>> {
    let re = compile_pattern(&cfg.pattern)?;

    // Phase 1: classify each line as matched (decoration + path + tail) or
    // unmatched (verbatim). Track original positions so unmatched headers
    // can be re-interleaved at the top.
    let mut matched: Vec<MatchedEntry> = Vec::new();
    let mut unmatched_prefix: Vec<String> = Vec::new();
    let mut seen_match = false;
    let mut unmatched_suffix: Vec<String> = Vec::new();
    for line in lines {
        if let Some(entry) = parse_line(&re, line) {
            seen_match = true;
            matched.push(entry);
        } else if cfg.passthrough_unmatched {
            if seen_match {
                unmatched_suffix.push((*line).to_string());
            } else {
                unmatched_prefix.push((*line).to_string());
            }
        }
    }

    if matched.len() < cfg.min_files {
        return None;
    }

    if shared_depth(&matched) < cfg.min_shared_depth {
        return None;
    }

    // Build trie
    let mut root = Node::new_dir(String::new());
    for entry in &matched {
        insert_path(&mut root, &entry.components, &entry.decoration, &entry.tail);
    }

    if cfg.collapse_single_child {
        collapse_root(&mut root);
    }

    if cfg.sort {
        sort_node(&mut root);
    }

    // Render
    let mut out = Vec::with_capacity(lines.len());
    out.extend(unmatched_prefix);
    render_root(&root, cfg.style, &mut out);
    out.extend(unmatched_suffix);
    Some(out)
}

/// A line that matched the tree pattern.
#[derive(Debug, Clone)]
struct MatchedEntry {
    /// Capture group 1 — the decoration (e.g. `"M  "`).
    decoration: String,
    /// Path components from capture group 2, split on `/`.
    components: Vec<String>,
    /// Trailing text after the path (e.g. ` -> new.rs` for renames).
    tail: String,
}

fn parse_line(re: &Regex, line: &str) -> Option<MatchedEntry> {
    let caps = re.captures(line)?;
    let decoration = caps.get(1).map_or("", |m| m.as_str()).to_string();
    let raw_path = caps.get(2)?.as_str();

    // Handle git rename arrow: split " -> " off and treat the suffix as a
    // leaf-tail decoration. The trie key uses the part *before* the arrow
    // (the original location).
    let (path, tail) = match raw_path.split_once(" -> ") {
        Some((before, after)) => (before, format!(" -> {after}")),
        None => (raw_path, String::new()),
    };

    let components: Vec<String> = path
        .split('/')
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect();

    if components.is_empty() {
        return None;
    }

    Some(MatchedEntry {
        decoration,
        components,
        tail,
    })
}

/// Number of leading path components shared across every matched entry.
fn shared_depth(matched: &[MatchedEntry]) -> usize {
    if matched.is_empty() {
        return 0;
    }
    let first = &matched[0].components;
    let mut depth = 0;
    'outer: for (i, comp) in first.iter().enumerate() {
        // Don't count the final component as a shared dir — that would
        // count duplicate filenames as "shared depth".
        if i == first.len() - 1 {
            break;
        }
        for entry in &matched[1..] {
            // If another entry has fewer components, the shared dir count
            // is at most i.
            if entry.components.len() <= i + 1 {
                break 'outer;
            }
            if entry.components[i] != *comp {
                break 'outer;
            }
        }
        depth = i + 1;
    }
    depth
}

// ───────────────────────── Trie ─────────────────────────

#[derive(Debug)]
struct Node {
    name: String,
    is_leaf: bool,
    /// For leaves: the decoration captured from the regex (e.g. `"M  "`).
    decoration: String,
    /// For leaves: any trailing text after the path (e.g. ` -> new.rs`).
    tail: String,
    children: Vec<Self>,
}

impl Node {
    const fn new_dir(name: String) -> Self {
        Self {
            name,
            is_leaf: false,
            decoration: String::new(),
            tail: String::new(),
            children: Vec::new(),
        }
    }

    const fn new_leaf(name: String, decoration: String, tail: String) -> Self {
        Self {
            name,
            is_leaf: true,
            decoration,
            tail,
            children: Vec::new(),
        }
    }
}

fn insert_path(root: &mut Node, components: &[String], decoration: &str, tail: &str) {
    if components.is_empty() {
        return;
    }
    if components.len() == 1 {
        root.children.push(Node::new_leaf(
            components[0].clone(),
            decoration.to_string(),
            tail.to_string(),
        ));
        return;
    }
    let head = &components[0];
    let rest = &components[1..];
    // Find existing dir child with this name (linear scan; n is small)
    let idx = if let Some(i) = root
        .children
        .iter()
        .position(|c| !c.is_leaf && c.name == *head)
    {
        i
    } else {
        root.children.push(Node::new_dir(head.clone()));
        root.children.len() - 1
    };
    insert_path(&mut root.children[idx], rest, decoration, tail);
}

fn collapse_root(root: &mut Node) {
    for child in &mut root.children {
        collapse_node(child);
    }
}

fn collapse_node(node: &mut Node) {
    // Recurse first
    for child in &mut node.children {
        collapse_node(child);
    }
    // Then collapse single-child chains at this node.
    while !node.is_leaf && node.children.len() == 1 {
        let mut grand = node.children.remove(0);
        node.name = format!("{}/{}", node.name, grand.name);
        if grand.is_leaf {
            node.is_leaf = true;
            node.decoration = grand.decoration;
            node.tail = grand.tail;
            break;
        }
        node.children = std::mem::take(&mut grand.children);
    }
}

fn sort_node(node: &mut Node) {
    node.children.sort_by(|a, b| a.name.cmp(&b.name));
    for child in &mut node.children {
        sort_node(child);
    }
}

// ───────────────────────── Rendering ─────────────────────────

#[derive(Debug, Clone, Copy)]
struct StyleChars {
    branch: &'static str,
    last: &'static str,
    vertical: &'static str,
    space: &'static str,
}

const UNICODE_CHARS: StyleChars = StyleChars {
    branch: "├─ ",
    last: "└─ ",
    vertical: "│  ",
    space: "   ",
};

const ASCII_CHARS: StyleChars = StyleChars {
    branch: "|- ",
    last: "`- ",
    vertical: "|  ",
    space: "   ",
};

const INDENT_CHARS: StyleChars = StyleChars {
    branch: "  ",
    last: "  ",
    vertical: "  ",
    space: "  ",
};

const fn style_chars(style: TreeStyle) -> StyleChars {
    match style {
        TreeStyle::Unicode => UNICODE_CHARS,
        TreeStyle::Ascii => ASCII_CHARS,
        TreeStyle::Indent => INDENT_CHARS,
    }
}

fn render_root(root: &Node, style: TreeStyle, out: &mut Vec<String>) {
    let chars = style_chars(style);
    // Top-level entries render flush left without connectors.
    for child in &root.children {
        render_top_level(child, chars, out);
    }
}

fn render_top_level(node: &Node, chars: StyleChars, out: &mut Vec<String>) {
    if node.is_leaf {
        out.push(format!("{}{}{}", node.decoration, node.name, node.tail));
        return;
    }
    // Top-level dir: just the dir name with trailing slash, flush left.
    out.push(format!("{}/", node.name));
    let n = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let is_last = i == n - 1;
        render_child(child, "", is_last, chars, out);
    }
}

fn render_child(
    node: &Node,
    prefix: &str,
    is_last: bool,
    chars: StyleChars,
    out: &mut Vec<String>,
) {
    let connector = if is_last { chars.last } else { chars.branch };
    if node.is_leaf {
        out.push(format!(
            "{}{}{}{}{}",
            prefix, connector, node.decoration, node.name, node.tail
        ));
        return;
    }
    out.push(format!("{}{}{}/", prefix, connector, node.name));
    let continuation = if is_last { chars.space } else { chars.vertical };
    let child_prefix = format!("{prefix}{continuation}");
    let n = node.children.len();
    for (i, child) in node.children.iter().enumerate() {
        let child_is_last = i == n - 1;
        render_child(child, &child_prefix, child_is_last, chars, out);
    }
}

// ───────────────────────── Regex caching ─────────────────────────

/// Cache compiled regexes by pattern string so the same `[tree]` config
/// doesn't re-compile its pattern on every call. The cache is mod-local
/// (the filter pipeline doesn't have a shared regex cache today —
/// `parse.rs` and `replace.rs` both call `Regex::new` directly per call).
/// If a third call site is added later this can be lifted into a shared
/// helper in `filter/mod.rs`.
fn compile_pattern(pattern: &str) -> Option<Regex> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    {
        let guard = cache.lock().ok()?;
        if let Some(re) = guard.get(pattern) {
            return Some(re.clone());
        }
    }
    let re = Regex::new(pattern).ok()?;
    cache.lock().ok()?.insert(pattern.to_string(), re.clone());
    Some(re)
}

// Tests live in `crate::filter::tests_tree_unit` (sibling file) so this
// module stays under the 500-line soft limit. Keeping the tests separate
// also guarantees they exercise only the public API of `tree::apply_tree`.
// End-to-end pipeline tests are in `tests_tree.rs`.
