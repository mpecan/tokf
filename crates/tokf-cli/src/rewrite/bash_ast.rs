//! AST-based bash command parsing using rable.
//!
//! Provides grammar-aware splitting, pipe detection, heredoc detection,
//! env-prefix stripping, and command word extraction.

use rable::ast::Node;

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
    ///
    /// Returns `None` if rable cannot parse the input.
    pub fn parse(source: &str) -> Option<Self> {
        let nodes = rable::parse(source, false).ok()?;
        Some(Self {
            nodes,
            source: source.to_string(),
        })
    }

    /// Split a compound command at chain operators (`&&`, `||`, `;`).
    ///
    /// Returns `(segment, separator)` pairs. Pipes are NOT separators.
    pub fn compound_segments(&self) -> Vec<(String, String)> {
        // Collect operator strings and their positions in source order.
        let operators = collect_operators(&self.nodes);
        if operators.is_empty() {
            return vec![(self.source.clone(), String::new())];
        }

        // Split source at operator positions.
        let mut result = Vec::new();
        let mut pos = 0;

        for op in &operators {
            if let Some(op_pos) = self.source[pos..].find(op.as_str()) {
                let abs_pos = pos + op_pos;
                let segment = self.source[pos..abs_pos].trim_end().to_string();
                let sep = rebuild_separator(&self.source, op, abs_pos);
                result.push((segment, sep));
                pos = abs_pos + op.len();
                // Skip whitespace after operator.
                while pos < self.source.len() && self.source.as_bytes()[pos] == b' ' {
                    pos += 1;
                }
            }
        }

        // Remaining text after last operator.
        let tail = self.source[pos..].to_string();
        result.push((tail, String::new()));

        result
    }

    /// Find byte positions of bare pipe operators (not `||`).
    pub fn pipe_positions(&self) -> Vec<usize> {
        let mut positions = Vec::new();
        for node in &self.nodes {
            collect_pipe_positions(&self.source, node, &mut positions);
        }
        positions
    }

    /// Returns `true` if the command contains at least one bare pipe.
    pub fn has_bare_pipe(&self) -> bool {
        self.nodes.iter().any(has_pipeline)
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
        // Find the first Command node and check for assignment words.
        let cmd = find_first_command(&self.nodes)?;
        let Node::Command { words, .. } = cmd else {
            return None;
        };

        // Count leading words that look like variable assignments (contain `=`).
        let mut assignment_count = 0;
        for word in words {
            if let Node::Word { value, .. } = word {
                if looks_like_assignment(value) {
                    assignment_count += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if assignment_count == 0 {
            return None;
        }

        // Find where the assignments end in the source string.
        // Walk through the source matching each assignment word.
        let mut pos = 0;
        for word in words.iter().take(assignment_count) {
            if let Node::Word { value, .. } = word
                && let Some(idx) = self.source[pos..].find(value.as_str())
            {
                pos += idx + value.len();
            }
        }

        // Include trailing whitespace.
        let rest_start = self.source[pos..]
            .find(|c: char| !c.is_ascii_whitespace())
            .map_or(self.source.len(), |offset| pos + offset);

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
        self.nodes.iter().any(|n| has_heredoc(n, false))
    }

    /// Extract command words (command name + arguments) from a simple command.
    #[cfg(test)]
    pub fn command_words(&self) -> Option<Vec<String>> {
        let cmd = find_first_command(&self.nodes)?;
        let Node::Command { words, .. } = cmd else {
            return None;
        };

        let result: Vec<String> = words
            .iter()
            .filter_map(|w| {
                if let Node::Word { value, .. } = w {
                    if looks_like_assignment(value) {
                        return None;
                    }
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

/// Collect operator strings from List nodes in order.
fn collect_operators(nodes: &[Node]) -> Vec<String> {
    let mut ops = Vec::new();
    for node in nodes {
        collect_operators_recursive(node, &mut ops);
    }
    ops
}

fn collect_operators_recursive(node: &Node, ops: &mut Vec<String>) {
    if let Node::List { parts } = node {
        for part in parts {
            if let Node::Operator { op } = part {
                ops.push(op.clone());
            } else {
                collect_operators_recursive(part, ops);
            }
        }
    }
}

/// Rebuild a separator string, preserving surrounding whitespace from the source.
fn rebuild_separator(source: &str, op: &str, op_pos: usize) -> String {
    let op_end = op_pos + op.len();
    let bytes = source.as_bytes();

    let before = if op_pos > 0 && bytes[op_pos - 1] == b' ' {
        " "
    } else {
        ""
    };
    let after = if op_end < bytes.len() && bytes[op_end] == b' ' {
        " "
    } else {
        ""
    };
    format!("{before}{op}{after}")
}

/// Find pipe byte positions in source from Pipeline nodes.
///
/// Since rable already identified the Pipeline structure, we know pipes exist.
/// We find their byte positions by locating each command's first word in sequence
/// and looking for `|` in the gap between adjacent commands.
fn collect_pipe_positions(source: &str, node: &Node, out: &mut Vec<usize>) {
    match node {
        Node::Pipeline { commands } => {
            // Find each command's approximate start by its first word.
            let mut cmd_starts: Vec<usize> = Vec::new();
            let mut search_from = 0;
            for cmd in commands {
                if let Some(word) = first_word_value(cmd)
                    && let Some(pos) = source[search_from..].find(word)
                {
                    let abs = search_from + pos;
                    cmd_starts.push(abs);
                    search_from = abs + word.len();
                }
            }
            // Find `|` between each pair of adjacent command starts.
            for window in cmd_starts.windows(2) {
                let gap = &source[window[0]..window[1]];
                // Search backwards from the second command to find the `|`.
                for (i, b) in gap.bytes().rev().enumerate() {
                    if b == b'|' {
                        let abs = window[1] - 1 - i;
                        // Ensure it's not `||`.
                        let prev_pipe = abs > 0 && source.as_bytes()[abs - 1] == b'|';
                        let next_pipe =
                            abs + 1 < source.len() && source.as_bytes()[abs + 1] == b'|';
                        if !prev_pipe && !next_pipe {
                            out.push(abs);
                            break;
                        }
                    }
                }
            }
        }
        Node::List { parts } => {
            for part in parts {
                collect_pipe_positions(source, part, out);
            }
        }
        _ => {}
    }
}

/// Extract the first word value from a Command node.
fn first_word_value(node: &Node) -> Option<&str> {
    if let Node::Command { words, .. } = node {
        for w in words {
            if let Node::Word { value, .. } = w {
                return Some(value.as_str());
            }
        }
    }
    None
}

/// Check if any node is or contains a Pipeline.
fn has_pipeline(node: &Node) -> bool {
    match node {
        Node::Pipeline { .. } => true,
        Node::List { parts } => parts.iter().any(has_pipeline),
        _ => false,
    }
}

/// Recursively check for top-level heredoc redirects.
fn has_heredoc(node: &Node, inside_substitution: bool) -> bool {
    match node {
        Node::HereDoc { .. } if !inside_substitution => true,
        Node::Redirect { op, .. }
            if !inside_substitution && op.starts_with("<<") && !op.starts_with("<<<") =>
        {
            true
        }
        Node::CommandSubstitution { command, .. } => has_heredoc(command, true),
        Node::Command { redirects, .. } => redirects
            .iter()
            .any(|r| has_heredoc(r, inside_substitution)),
        Node::Pipeline { commands } => commands.iter().any(|c| has_heredoc(c, inside_substitution)),
        Node::List { parts } => parts.iter().any(|p| has_heredoc(p, inside_substitution)),
        _ => false,
    }
}

/// Find the first Command node in the AST.
fn find_first_command(nodes: &[Node]) -> Option<&Node> {
    for node in nodes {
        match node {
            Node::Command { .. } => return Some(node),
            Node::Pipeline { commands } => {
                if let Some(cmd) = find_first_command(commands) {
                    return Some(cmd);
                }
            }
            Node::List { parts } => {
                if let Some(cmd) = find_first_command(parts) {
                    return Some(cmd);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if a word looks like a variable assignment (`KEY=VALUE`).
fn looks_like_assignment(value: &str) -> bool {
    value.find('=').is_some_and(|eq_pos| {
        let name = &value[..eq_pos];
        !name.is_empty() && (name.as_bytes()[0].is_ascii_alphabetic() || name.as_bytes()[0] == b'_')
    })
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
