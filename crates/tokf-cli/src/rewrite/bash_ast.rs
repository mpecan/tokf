//! AST-based bash command parsing using rable.
//!
//! Provides grammar-aware splitting, pipe detection, heredoc detection,
//! env-prefix stripping, and command word extraction.

use rable::ast::{ListOperator, Node, NodeKind};

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
pub fn has_toplevel_heredoc(command: &str) -> bool {
    ParsedCommand::parse(command).is_some_and(|p| p.has_toplevel_heredoc())
}

/// A parsed bash command backed by a rable AST.
pub struct ParsedCommand {
    nodes: Vec<Node>,
    source: String,
}

impl ParsedCommand {
    /// Parse a bash command string into an AST.
    pub fn parse(source: &str) -> Option<Self> {
        let nodes = rable::parse(source, false).ok()?;
        Some(Self {
            nodes,
            source: source.to_string(),
        })
    }

    fn text(&self, node: &Node) -> &str {
        node.source_text(&self.source)
    }

    /// Split a compound command at chain operators.
    pub fn compound_segments(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();

        for node in &self.nodes {
            if let NodeKind::List { items } = &node.kind {
                for item in items {
                    let text = self.text(&item.command).to_string();
                    let sep = item
                        .operator
                        .map(|op| format_operator(&self.source, op, &item.command))
                        .unwrap_or_default();
                    result.push((text, sep));
                }
            } else {
                result.push((self.text(node).to_string(), String::new()));
            }
        }

        if result.is_empty() {
            return vec![(self.source.clone(), String::new())];
        }
        result
    }

    /// Find byte positions of bare pipe operators.
    pub fn pipe_positions(&self) -> Vec<usize> {
        let mut positions = Vec::new();
        for node in &self.nodes {
            collect_pipe_positions(&self.source, node, &mut positions);
        }
        positions
    }

    /// Returns `true` if the command contains at least one bare pipe.
    pub fn has_bare_pipe(&self) -> bool {
        self.nodes.iter().any(|n| has_pipeline(&n.kind))
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
    pub fn env_prefix(&self) -> Option<(String, String)> {
        let cmd = find_first_command(&self.nodes)?;
        let NodeKind::Command { assignments, .. } = &cmd.kind else {
            return None;
        };
        if assignments.is_empty() {
            return None;
        }

        let last = assignments.last()?;
        let prefix_text_end = last.source_text(&self.source);
        // Find the byte position after the last assignment's text.
        let prefix_end_byte = self.source.find(prefix_text_end)? + prefix_text_end.len();

        // Include trailing whitespace.
        let rest_start = self.source[prefix_end_byte..]
            .find(|c: char| !c.is_ascii_whitespace())
            .map_or(self.source.len(), |offset| prefix_end_byte + offset);

        let rest = &self.source[rest_start..];
        if rest.is_empty() {
            return None;
        }

        Some((self.source[..rest_start].to_string(), rest.to_string()))
    }

    /// Detect whether the command has a top-level heredoc redirect.
    pub fn has_toplevel_heredoc(&self) -> bool {
        self.nodes.iter().any(|n| has_heredoc(n, false))
    }

    /// Extract command words (command name + arguments) from a simple command.
    #[cfg(test)]
    pub fn command_words(&self) -> Option<Vec<String>> {
        let cmd = find_first_command(&self.nodes)?;
        let NodeKind::Command { words, .. } = &cmd.kind else {
            return None;
        };
        let result: Vec<String> = words
            .iter()
            .filter_map(|w| {
                if let NodeKind::Word { value, .. } = &w.kind {
                    Some(unquote(value))
                } else {
                    None
                }
            })
            .collect();
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}

// --- AST walking helpers ---

/// Format the operator after a list item, preserving surrounding whitespace.
fn format_operator(source: &str, op: ListOperator, cmd: &Node) -> String {
    let op_str = match op {
        ListOperator::And => "&&",
        ListOperator::Or => "||",
        ListOperator::Semi => ";",
        ListOperator::Background => "&",
    };

    // Find the operator in the source after the command's text.
    let cmd_text = cmd.source_text(source);
    let search_from = source.find(cmd_text).unwrap_or(0) + cmd_text.len();
    source[search_from..].find(op_str).map_or_else(
        || format!(" {op_str} "),
        |pos| {
            let abs = search_from + pos;
            let abs_end = abs + op_str.len();
            let bytes = source.as_bytes();
            let before = if abs > 0 && bytes[abs - 1] == b' ' {
                " "
            } else {
                ""
            };
            let after = if abs_end < bytes.len() && bytes[abs_end] == b' ' {
                " "
            } else {
                ""
            };
            format!("{before}{op_str}{after}")
        },
    )
}

/// Collect pipe byte positions from Pipeline nodes.
///
/// Uses the gap between adjacent pipeline commands (via `source_text`)
/// to locate the `|` character in the source.
fn collect_pipe_positions(source: &str, node: &Node, out: &mut Vec<usize>) {
    match &node.kind {
        NodeKind::Pipeline { commands, .. } => {
            for pair in commands.windows(2) {
                let left_text = pair[0].source_text(source);
                let right_text = pair[1].source_text(source);
                // Find byte positions of each command in source.
                let left_end = source.find(left_text).unwrap_or(0) + left_text.len();
                let right_start = source[left_end..].find(right_text).unwrap_or(0) + left_end;
                // Scan the gap for `|`.
                for (i, b) in source[left_end..right_start].bytes().enumerate() {
                    if b == b'|' {
                        let abs = left_end + i;
                        let bytes = source.as_bytes();
                        let prev = abs > 0 && bytes[abs - 1] == b'|';
                        let next = abs + 1 < bytes.len() && bytes[abs + 1] == b'|';
                        if !prev && !next {
                            out.push(abs);
                            break;
                        }
                    }
                }
            }
        }
        NodeKind::List { items } => {
            for item in items {
                collect_pipe_positions(source, &item.command, out);
            }
        }
        _ => {}
    }
}

/// Check if a node kind is or contains a Pipeline.
fn has_pipeline(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Pipeline { .. } => true,
        NodeKind::List { items } => items.iter().any(|item| has_pipeline(&item.command.kind)),
        _ => false,
    }
}

/// Recursively check for top-level heredoc redirects.
fn has_heredoc(node: &Node, inside_substitution: bool) -> bool {
    match &node.kind {
        NodeKind::HereDoc { .. } if !inside_substitution => true,
        NodeKind::Redirect { op, .. }
            if !inside_substitution && op.starts_with("<<") && !op.starts_with("<<<") =>
        {
            true
        }
        NodeKind::CommandSubstitution { command, .. } => has_heredoc(command, true),
        NodeKind::Command { redirects, .. } => redirects
            .iter()
            .any(|r| has_heredoc(r, inside_substitution)),
        NodeKind::Pipeline { commands, .. } => {
            commands.iter().any(|c| has_heredoc(c, inside_substitution))
        }
        NodeKind::List { items } => items
            .iter()
            .any(|item| has_heredoc(&item.command, inside_substitution)),
        _ => false,
    }
}

/// Find the first Command node in the AST.
fn find_first_command(nodes: &[Node]) -> Option<&Node> {
    for node in nodes {
        match &node.kind {
            NodeKind::Command { .. } => return Some(node),
            NodeKind::Pipeline { commands, .. } => {
                if let Some(cmd) = find_first_command(commands) {
                    return Some(cmd);
                }
            }
            NodeKind::List { items } => {
                for item in items {
                    if let Some(cmd) = find_first_command(std::slice::from_ref(&item.command)) {
                        return Some(cmd);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

// --- Pipe strippability checks (tokf-specific policy, not parsing) ---

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

#[cfg(test)]
fn unquote(s: &str) -> String {
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

// Tests are in bash_ast_tests.rs (registered in mod.rs).
