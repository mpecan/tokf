//! AST-based bash command parsing using tree-sitter-bash.
//!
//! Provides grammar-aware splitting, pipe detection, heredoc detection,
//! env-prefix stripping, and command word extraction.

use tree_sitter::{Node, Parser, Tree};

/// Result of stripping a simple pipe from a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrippedPipe {
    /// The base command with the pipe removed (e.g. "cargo test").
    pub base: String,
    /// The raw pipe suffix (e.g. "tail -5", "grep FAIL").
    pub suffix: String,
}

// --- Public API ---

/// Split a compound shell command at chain operators (`&&`, `||`, `;`, newline).
pub fn split_compound(input: &str) -> Vec<(String, String)> {
    ParsedCommand::parse(input).map_or_else(
        || vec![(input.to_string(), String::new())],
        |p| p.compound_segments(),
    )
}

/// Find byte positions of bare pipe operators (not `||`).
pub fn pipe_positions(command: &str) -> Vec<usize> {
    ParsedCommand::parse(command).map_or_else(Vec::new, |p| p.pipe_positions())
}

/// Strip leading environment variable assignments from a command.
pub fn strip_env_prefix(command: &str) -> Option<(String, String)> {
    ParsedCommand::parse(command)?.env_prefix()
}

/// Returns `true` if the command has a top-level heredoc redirect.
///
/// Uses the AST when the input is a complete heredoc (body included).
/// Falls back to scanning for `<<` at depth 0 for incomplete commands
/// (e.g. `cat <<EOF` without the body), since tree-sitter needs the
/// full heredoc syntax to produce a `heredoc_redirect` node.
pub fn has_toplevel_heredoc(command: &str) -> bool {
    if let Some(p) = ParsedCommand::parse(command) {
        if p.has_toplevel_heredoc() {
            return true;
        }
        // Check ERROR nodes for `<<` — indicates an incomplete heredoc.
        if scan_for_heredoc_marker(p.tree.root_node(), &p.source, false) {
            return true;
        }
    }
    false
}

fn scan_for_heredoc_marker(node: Node, source: &str, inside_substitution: bool) -> bool {
    if node.kind() == "ERROR" && !inside_substitution {
        let text = &source[node.byte_range()];
        if text.contains("<<") {
            return true;
        }
    }
    let in_sub = inside_substitution || node.kind() == "command_substitution";
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if scan_for_heredoc_marker(child, source, in_sub) {
            return true;
        }
    }
    false
}

/// A parsed bash command backed by a tree-sitter AST.
pub struct ParsedCommand {
    tree: Tree,
    source: String,
}

impl ParsedCommand {
    /// Parse a bash command string into an AST.
    ///
    /// Returns `None` if tree-sitter cannot parse the input at all.
    pub fn parse(source: &str) -> Option<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .ok()?;
        let tree = parser.parse(source, None)?;
        Some(Self {
            tree,
            source: source.to_string(),
        })
    }

    /// Text slice for an AST node.
    #[cfg(test)]
    fn text(&self, node: Node) -> &str {
        &self.source[node.byte_range()]
    }

    /// Split a compound command at chain operators (`&&`, `||`, `;`).
    ///
    /// Returns `(segment, separator)` pairs matching the legacy
    /// `compound::split_compound` API. Pipes are NOT separators.
    pub fn compound_segments(&self) -> Vec<(String, String)> {
        let root = self.tree.root_node();
        let mut result = Vec::new();
        collect_segments(&self.source, root, &mut result);

        if result.is_empty() {
            return vec![(self.source.clone(), String::new())];
        }
        result
    }

    /// Find byte positions of bare pipe operators (not `||`).
    pub fn pipe_positions(&self) -> Vec<usize> {
        let root = self.tree.root_node();
        let mut positions = Vec::new();
        collect_pipe_positions(root, &mut positions);
        positions
    }

    /// Returns `true` if the command contains at least one bare pipe.
    pub fn has_bare_pipe(&self) -> bool {
        !self.pipe_positions().is_empty()
    }

    /// If the command has exactly one bare pipe to a simple suffix
    /// (tail/head/grep), return the base and suffix.
    pub fn strip_simple_pipe(&self) -> Option<StrippedPipe> {
        let positions = self.pipe_positions();
        if positions.len() != 1 {
            return None;
        }
        let pipe_pos = positions[0];
        let suffix = self.source[pipe_pos + 1..].trim();
        if is_strippable_suffix(suffix) {
            Some(StrippedPipe {
                base: self.source[..pipe_pos].trim_end().to_string(),
                suffix: suffix.to_string(),
            })
        } else {
            None
        }
    }

    /// Extract leading environment variable assignments.
    ///
    /// Returns `Some((env_prefix, rest))` where `env_prefix` includes
    /// trailing whitespace. Returns `None` if there are no env var assignments.
    pub fn env_prefix(&self) -> Option<(String, String)> {
        let root = self.tree.root_node();
        let cmd_node = find_first_command(root)?;

        let mut last_assignment_end = 0;
        let mut found_assignment = false;
        let mut cursor = cmd_node.walk();

        for child in cmd_node.children(&mut cursor) {
            if child.kind() == "variable_assignment" {
                last_assignment_end = child.end_byte();
                found_assignment = true;
            } else {
                break;
            }
        }

        if !found_assignment {
            return None;
        }

        let rest_start = self.source[last_assignment_end..]
            .find(|c: char| !c.is_ascii_whitespace())
            .map_or(self.source.len(), |offset| last_assignment_end + offset);

        let prefix = &self.source[..rest_start];
        let rest = &self.source[rest_start..];

        if rest.is_empty() {
            return None;
        }

        Some((prefix.to_string(), rest.to_string()))
    }

    /// Detect whether the command has a top-level heredoc redirect.
    ///
    /// A heredoc inside `$(...)` command substitution is NOT top-level.
    pub fn has_toplevel_heredoc(&self) -> bool {
        let root = self.tree.root_node();
        find_toplevel_heredoc(root, false)
    }

    /// Extract command words (command name + arguments) from a simple command.
    ///
    /// Strips quotes from word nodes. Useful for pattern matching.
    #[cfg(test)]
    pub fn command_words(&self) -> Option<Vec<String>> {
        let root = self.tree.root_node();
        let cmd_node = find_first_command(root)?;
        let mut words = Vec::new();
        let mut cursor = cmd_node.walk();

        for child in cmd_node.children(&mut cursor) {
            match child.kind() {
                "command_name" | "word" | "string" | "raw_string" | "concatenation"
                | "simple_expansion" | "expansion" | "number" => {
                    words.push(unquote(self.text(child)));
                }
                "variable_assignment" => {} // skip env vars
                _ => {
                    if child.is_named() {
                        words.push(self.text(child).to_string());
                    }
                }
            }
        }

        if words.is_empty() { None } else { Some(words) }
    }
}

// --- Free functions for tree walking (avoids clippy::only_used_in_recursion) ---

/// Recursively collect segments from compound nodes.
fn collect_segments(source: &str, node: Node, out: &mut Vec<(String, String)>) {
    match node.kind() {
        "list" | "program" => {
            let child_count = node.child_count();
            for i in 0..child_count {
                let Some(child) = node.child(i) else {
                    continue;
                };
                let kind = child.kind();
                if is_chain_operator(kind) || kind == "\n" {
                    let sep = if kind == "\n" {
                        "\n".to_string()
                    } else {
                        rebuild_separator(source, &child)
                    };
                    if let Some(last) = out.last_mut() {
                        last.1 = sep;
                    }
                } else {
                    collect_segments(source, child, out);
                }
            }
        }
        _ => {
            out.push((source[node.byte_range()].to_string(), String::new()));
        }
    }
}

/// Rebuild the separator string preserving surrounding whitespace.
fn rebuild_separator(source: &str, node: &Node) -> String {
    let op = &source[node.byte_range()];
    let start = node.start_byte();
    let end = node.end_byte();
    let bytes = source.as_bytes();

    let before = if start > 0 && bytes[start - 1] == b' ' {
        " "
    } else {
        ""
    };
    let after = if end < bytes.len() && bytes[end] == b' ' {
        " "
    } else {
        ""
    };
    format!("{before}{op}{after}")
}

/// Collect byte-offsets of `|` operators from pipeline nodes.
fn collect_pipe_positions(node: Node, out: &mut Vec<usize>) {
    if node.kind() == "pipeline" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "|" {
                out.push(child.start_byte());
            }
        }
        return; // Don't recurse further — pipes already collected.
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_pipe_positions(child, out);
    }
}

/// Check if a node kind is a chain operator.
fn is_chain_operator(kind: &str) -> bool {
    kind == "&&" || kind == "||" || kind == ";"
}

/// Find the first `command` node in the AST (depth-first).
fn find_first_command(node: Node) -> Option<Node> {
    if node.kind() == "command" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(cmd) = find_first_command(child) {
            return Some(cmd);
        }
    }
    None
}

/// Recursively check for top-level heredoc redirects.
fn find_toplevel_heredoc(node: Node, inside_substitution: bool) -> bool {
    if node.kind() == "heredoc_redirect" && !inside_substitution {
        return true;
    }
    let in_sub = inside_substitution || node.kind() == "command_substitution";
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if find_toplevel_heredoc(child, in_sub) {
            return true;
        }
    }
    false
}

// --- Pipe strippability checks (tokf-specific policy, not parsing) ---

/// Check whether the text after a pipe is a simple truncation/filter target
/// that tokf's structured output can replace.
fn is_strippable_suffix(suffix: &str) -> bool {
    let mut words = suffix.split_whitespace();
    let Some(cmd) = words.next() else {
        return false;
    };
    let args: Vec<&str> = words.collect();
    match cmd {
        "tail" | "head" => is_strippable_tail_head(&args),
        "grep" => is_strippable_grep(&args),
        _ => false,
    }
}

/// Accept tail/head with no args or line-selection flags only.
/// Reject -f (follow), -c/--bytes (byte mode), filenames, and unknown flags.
fn is_strippable_tail_head(args: &[&str]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i];
        if arg == "-f" || arg.starts_with("-c") || arg.starts_with("--bytes") {
            return false;
        }
        if arg == "-n" || arg == "--lines" {
            i += 2;
            continue;
        }
        if arg.starts_with("-n") || arg.starts_with("--lines=") {
            i += 1;
            continue;
        }
        if arg.starts_with('-')
            && arg.len() > 1
            && arg.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        {
            i += 1;
            continue;
        }
        return false;
    }
    true
}

/// Accept grep with allowed filter flags and at least one pattern argument.
/// Reject -c (count), -l/-L (file listing), long flags, and unknown short flags.
fn is_strippable_grep(args: &[&str]) -> bool {
    const ALLOWED: &[u8] = b"iEFwvx";
    let mut has_pattern = false;
    for arg in args {
        if arg.starts_with("--") {
            return false;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if !arg.as_bytes()[1..].iter().all(|b| ALLOWED.contains(b)) {
                return false;
            }
        } else {
            has_pattern = true;
        }
    }
    has_pattern
}

/// Remove surrounding quotes from a string.
#[cfg(test)]
fn unquote(s: &str) -> String {
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

// Tests are in bash_ast_tests.rs (registered in mod.rs).
