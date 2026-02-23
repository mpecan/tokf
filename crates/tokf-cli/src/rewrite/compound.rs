use regex::Regex;

/// Matches shell chain operators (`&&`, `||`, `;`, newline). Pipes excluded intentionally.
const CHAIN_PATTERN: &str = r"( *&& *| *\|\| *| *; *|\n)";

/// Split a compound shell command at chain operators (`&&`, `||`, `;`, newline).
///
/// Returns `(segment, separator)` pairs; the last separator is always `""`.
/// Pipes (`|`) are not treated as chain operators — `tokf run cmd | head` is valid
/// shell and lets the outer shell pass tokf's filtered output through the pipe.
pub fn split_compound(input: &str) -> Vec<(String, String)> {
    let Ok(re) = Regex::new(CHAIN_PATTERN) else {
        return vec![(input.to_string(), String::new())];
    };
    if !re.is_match(input) {
        return vec![(input.to_string(), String::new())];
    }
    let mut parts = Vec::new();
    let mut last = 0;
    for m in re.find_iter(input) {
        parts.push((input[last..m.start()].to_string(), m.as_str().to_string()));
        last = m.end();
    }
    if last <= input.len() {
        parts.push((input[last..].to_string(), String::new()));
    }
    parts
}

/// Result of stripping a simple pipe from a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrippedPipe {
    /// The base command with the pipe removed (e.g. "cargo test").
    pub base: String,
    /// The raw pipe suffix (e.g. "tail -5", "grep FAIL").
    pub suffix: String,
}

/// If the command has exactly one bare pipe whose target is simple output
/// truncation or filtering (tail, head, grep), return the base command
/// and pipe suffix. Returns `None` for multi-pipe chains, pipes to
/// other commands, or tail/head with non-line-selection flags (-f, -c).
pub fn strip_simple_pipe(command: &str) -> Option<StrippedPipe> {
    let positions = bare_pipe_positions(command);
    if positions.len() != 1 {
        return None;
    }

    let pipe_pos = positions[0];
    let suffix = command[pipe_pos + 1..].trim();

    if is_strippable_suffix(suffix) {
        Some(StrippedPipe {
            base: command[..pipe_pos].trim_end().to_string(),
            suffix: suffix.to_string(),
        })
    } else {
        None
    }
}

/// Collect the byte-offsets of every bare pipe (`|`) that is not part of `||`.
/// Quote-aware: pipes inside single or double quotes are ignored.
fn bare_pipe_positions(command: &str) -> Vec<usize> {
    let bytes = command.as_bytes();
    let mut positions = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\'' {
                in_single = false;
            }
        } else if in_double {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 1;
            } else if b == b'"' {
                in_double = false;
            }
        } else {
            match b {
                b'\'' => in_single = true,
                b'"' => in_double = true,
                b'|' => {
                    let prev_pipe = i > 0 && bytes[i - 1] == b'|';
                    let next_pipe = i + 1 < bytes.len() && bytes[i + 1] == b'|';
                    if !prev_pipe && !next_pipe {
                        positions.push(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    positions
}

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
            i += 2; // skip the flag and its numeric value
            continue;
        }
        if arg.starts_with("-n") || arg.starts_with("--lines=") {
            i += 1;
            continue;
        }
        // Bare numeric like -5, -10
        if arg.starts_with('-')
            && arg.len() > 1
            && arg.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        {
            i += 1;
            continue;
        }
        // Unrecognised flag or filename — not strippable
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

/// Returns `true` if `command` contains a bare pipe (`|`) not part of `||`.
///
/// Tracks single- and double-quote state so pipes inside quoted strings (e.g.
/// `grep -E 'foo|bar'` or `echo "a | b"`) are not counted as shell pipe operators.
/// Backslash escapes inside double-quoted strings are honoured.
///
/// Used to skip auto-rewriting commands where downstream processing (e.g. `grep`,
/// `wc -l`, `tee`) depends on the raw output. Note: user-configured rewrite rules
/// run before this check and can still wrap piped commands explicitly.
pub fn has_bare_pipe(command: &str) -> bool {
    !bare_pipe_positions(command).is_empty()
}

/// Strip leading shell environment variable assignments from a command.
///
/// Returns `Some((env_prefix, rest))` where `env_prefix` includes the trailing
/// whitespace (e.g. `"FOO=bar "`) and `rest` is the actual executable command.
/// Returns `None` if no `KEY=VALUE` tokens precede the command.
///
/// Handles unquoted, single-quoted, and double-quoted values so that
/// `FOO='bar baz' git status` correctly strips `FOO='bar baz' ` rather than
/// stopping at the space inside the quoted value.
///
/// This mirrors how [`strip_simple_pipe`] is handled: env vars are ignored when
/// determining whether a command matches a filter, but they are preserved in the
/// rewritten output and applied to the command that runs.
///
/// # Examples
/// - `"FOO=bar git status"` → `Some(("FOO=bar ", "git status"))`
/// - `"A=1 B=2 cargo test"` → `Some(("A=1 B=2 ", "cargo test"))`
/// - `"git status"` → `None`
pub fn strip_env_prefix(command: &str) -> Option<(String, String)> {
    // One env-var token: KEY=VALUE where VALUE is zero or more fragments:
    //   - unquoted non-special chars:   [^\s\\'"]+   (excludes backslash explicitly)
    //   - backslash-escape pair:        \\.           (handles FOO=bar\ baz and the '\'' idiom)
    //   - single-quoted section:        '[^']*'
    //   - double-quoted section:        "(?:[^"\\]|\\.)*"
    // One or more such tokens, each followed by horizontal whitespace.
    let Ok(re) = Regex::new(
        r#"^((?:[A-Za-z_][A-Za-z0-9_]*=(?:[^\s\\'"]+|\\.|'[^']*'|"(?:[^"\\]|\\.)*")*[ \t]+)+)"#,
    ) else {
        return None;
    };
    let caps = re.captures(command)?;
    let prefix = caps.get(1)?.as_str();
    if prefix.is_empty() {
        return None;
    }
    Some((prefix.to_string(), command[prefix.len()..].to_string()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn single_segment_when_no_operator() {
        // Pipes are NOT chain operators — the whole string is one segment.
        let parts = split_compound("git diff HEAD | head -5");
        assert_eq!(
            parts,
            vec![("git diff HEAD | head -5".to_string(), String::new())]
        );
    }

    #[test]
    fn splits_and_then_semicolon() {
        let parts = split_compound("git add foo && git diff; git status");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], ("git add foo".to_string(), " && ".to_string()));
        assert_eq!(parts[1], ("git diff".to_string(), "; ".to_string()));
        assert_eq!(parts[2], ("git status".to_string(), String::new()));
    }

    #[test]
    fn splits_or_operator() {
        let parts = split_compound("make test || cargo test");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "make test");
        assert_eq!(parts[1].0, "cargo test");
    }

    #[test]
    fn splits_newline() {
        let parts = split_compound("git add .\ngit status");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, "git add .");
        assert_eq!(parts[1].0, "git status");
    }

    #[test]
    fn has_bare_pipe_single_pipe() {
        assert!(has_bare_pipe("git diff HEAD | head -5"));
    }

    #[test]
    fn has_bare_pipe_multi_pipe_chain() {
        assert!(has_bare_pipe("cmd | grep foo | wc -l"));
    }

    #[test]
    fn has_bare_pipe_logical_or_only() {
        assert!(!has_bare_pipe("make test || cargo test"));
    }

    #[test]
    fn has_bare_pipe_no_pipe() {
        assert!(!has_bare_pipe("cargo build --release"));
    }

    #[test]
    fn has_bare_pipe_mixed_or_and_pipe() {
        assert!(has_bare_pipe("a || b | c"));
    }

    // --- quote-awareness ---

    #[test]
    fn has_bare_pipe_pipe_in_single_quotes_ignored() {
        assert!(!has_bare_pipe("grep -E 'foo|bar' file.txt"));
    }

    #[test]
    fn has_bare_pipe_pipe_in_double_quotes_ignored() {
        assert!(!has_bare_pipe(r#"echo "a | b""#));
    }

    #[test]
    fn has_bare_pipe_escaped_quote_does_not_end_double_quote() {
        // The \" inside the string does NOT close the double-quote context,
        // so the | remains inside quotes and is not a bare pipe.
        assert!(!has_bare_pipe(r#"echo "foo \" | bar""#));
    }

    #[test]
    fn has_bare_pipe_pipe_after_closing_quote_is_bare() {
        // The pipe is outside the quotes — it IS a bare pipe.
        assert!(has_bare_pipe(r#"echo "hello" | grep o"#));
    }

    // --- edge cases ---

    #[test]
    fn has_bare_pipe_empty_string() {
        assert!(!has_bare_pipe(""));
    }

    #[test]
    fn has_bare_pipe_only_pipe() {
        assert!(has_bare_pipe("|"));
    }

    #[test]
    fn has_bare_pipe_bash_stderr_pipe() {
        // |& is Bash's pipe-stderr shorthand; the leading | is still a bare pipe.
        assert!(has_bare_pipe("cargo test |& tee log.txt"));
    }

    // --- strip_simple_pipe ---

    fn stripped(base: &str, suffix: &str) -> StrippedPipe {
        StrippedPipe {
            base: base.to_string(),
            suffix: suffix.to_string(),
        }
    }

    #[test]
    fn strip_tail_n() {
        assert_eq!(
            strip_simple_pipe("cargo test | tail -n 5"),
            Some(stripped("cargo test", "tail -n 5"))
        );
    }

    #[test]
    fn strip_tail_numeric() {
        assert_eq!(
            strip_simple_pipe("cargo test | tail -5"),
            Some(stripped("cargo test", "tail -5"))
        );
    }

    #[test]
    fn strip_tail_bare() {
        assert_eq!(
            strip_simple_pipe("cargo test | tail"),
            Some(stripped("cargo test", "tail"))
        );
    }

    #[test]
    fn strip_head_n() {
        assert_eq!(
            strip_simple_pipe("cargo test | head -n 10"),
            Some(stripped("cargo test", "head -n 10"))
        );
    }

    #[test]
    fn strip_head_bare() {
        assert_eq!(
            strip_simple_pipe("cargo test | head"),
            Some(stripped("cargo test", "head"))
        );
    }

    #[test]
    fn strip_tail_lines_long() {
        assert_eq!(
            strip_simple_pipe("cargo test | tail --lines=5"),
            Some(stripped("cargo test", "tail --lines=5"))
        );
    }

    #[test]
    fn strip_grep_pattern() {
        assert_eq!(
            strip_simple_pipe("cargo test | grep FAIL"),
            Some(stripped("cargo test", "grep FAIL"))
        );
    }

    #[test]
    fn strip_grep_case_insensitive() {
        assert_eq!(
            strip_simple_pipe("cargo test | grep -i error"),
            Some(stripped("cargo test", "grep -i error"))
        );
    }

    #[test]
    fn strip_grep_extended() {
        assert_eq!(
            strip_simple_pipe("cargo test | grep -E 'fail|error'"),
            Some(stripped("cargo test", "grep -E 'fail|error'"))
        );
    }

    #[test]
    fn strip_grep_invert() {
        assert_eq!(
            strip_simple_pipe("cargo test | grep -v noise"),
            Some(stripped("cargo test", "grep -v noise"))
        );
    }

    #[test]
    fn no_strip_tail_follow() {
        assert_eq!(strip_simple_pipe("cargo test | tail -f"), None);
    }

    #[test]
    fn no_strip_tail_bytes() {
        assert_eq!(strip_simple_pipe("cargo test | tail -c 100"), None);
    }

    #[test]
    fn no_strip_head_bytes() {
        assert_eq!(strip_simple_pipe("cargo test | head -c 50"), None);
    }

    #[test]
    fn no_strip_grep_count() {
        assert_eq!(strip_simple_pipe("cargo test | grep -c FAIL"), None);
    }

    #[test]
    fn no_strip_grep_files() {
        assert_eq!(strip_simple_pipe("cargo test | grep -l FAIL"), None);
    }

    #[test]
    fn no_strip_wc() {
        assert_eq!(strip_simple_pipe("cargo test | wc -l"), None);
    }

    #[test]
    fn no_strip_sort() {
        assert_eq!(strip_simple_pipe("cargo test | sort"), None);
    }

    #[test]
    fn no_strip_multi_pipe() {
        assert_eq!(strip_simple_pipe("cmd | grep foo | tail -5"), None);
    }

    #[test]
    fn strip_quoted_pipe_in_base() {
        // The pipe inside the quotes is not a bare pipe; the real pipe is to tail.
        assert_eq!(
            strip_simple_pipe("grep 'a|b' | tail -5"),
            Some(stripped("grep 'a|b'", "tail -5"))
        );
    }

    #[test]
    fn no_strip_multi_pipe_with_tail() {
        assert_eq!(strip_simple_pipe("cargo test | tail -n 5 | grep x"), None);
    }

    #[test]
    fn no_strip_grep_no_pattern() {
        // grep with only flags but no pattern argument is not strippable.
        assert_eq!(strip_simple_pipe("cargo test | grep -i"), None);
    }

    #[test]
    fn strip_grep_combined_flags() {
        assert_eq!(
            strip_simple_pipe("cargo test | grep -iv error"),
            Some(stripped("cargo test", "grep -iv error"))
        );
    }

    #[test]
    fn strip_head_lines_long_with_space() {
        // --lines with a space separator (not =)
        assert_eq!(
            strip_simple_pipe("cargo test | head --lines 10"),
            Some(stripped("cargo test", "head --lines 10"))
        );
    }

    #[test]
    fn no_strip_empty_suffix() {
        // Trailing pipe with nothing after it.
        assert_eq!(strip_simple_pipe("cargo test |"), None);
    }

    #[test]
    fn no_strip_grep_uppercase_l() {
        // -L is "files without match" — changes output format.
        assert_eq!(strip_simple_pipe("cargo test | grep -L FAIL"), None);
    }

    // --- strip_env_prefix ---

    #[test]
    fn env_prefix_single_var() {
        assert_eq!(
            strip_env_prefix("FOO=bar git status"),
            Some(("FOO=bar ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_multiple_vars() {
        assert_eq!(
            strip_env_prefix("A=1 B=2 cargo test"),
            Some(("A=1 B=2 ".to_string(), "cargo test".to_string()))
        );
    }

    #[test]
    fn env_prefix_empty_value() {
        assert_eq!(
            strip_env_prefix("FOO= git status"),
            Some(("FOO= ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_single_quoted_value() {
        assert_eq!(
            strip_env_prefix("FOO='bar baz' git status"),
            Some(("FOO='bar baz' ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_double_quoted_value() {
        assert_eq!(
            strip_env_prefix(r#"FOO="bar baz" git status"#),
            Some((r#"FOO="bar baz" "#.to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_none_for_plain_command() {
        assert_eq!(strip_env_prefix("git status"), None);
    }

    #[test]
    fn env_prefix_none_for_command_with_flags() {
        assert_eq!(strip_env_prefix("cargo test --lib"), None);
    }

    #[test]
    fn env_prefix_underscore_key() {
        assert_eq!(
            strip_env_prefix("_MY_VAR=1 make"),
            Some(("_MY_VAR=1 ".to_string(), "make".to_string()))
        );
    }

    #[test]
    fn env_prefix_real_world_rust() {
        assert_eq!(
            strip_env_prefix("RUST_LOG=debug CARGO_TERM_COLOR=always cargo test"),
            Some((
                "RUST_LOG=debug CARGO_TERM_COLOR=always ".to_string(),
                "cargo test".to_string()
            ))
        );
    }

    #[test]
    fn env_prefix_equals_in_value() {
        // Values containing '=' are valid and common (e.g. key-value pairs passed as env).
        assert_eq!(
            strip_env_prefix("FOO=a=b git status"),
            Some(("FOO=a=b ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_long_path_value() {
        // PATH-style values with colons are entirely unquoted non-whitespace chars.
        assert_eq!(
            strip_env_prefix("PATH=/usr/local/bin:/usr/bin:/bin git status"),
            Some((
                "PATH=/usr/local/bin:/usr/bin:/bin ".to_string(),
                "git status".to_string()
            ))
        );
    }

    #[test]
    fn env_prefix_backslash_escaped_space() {
        // FOO=bar\ baz is a single env var whose value contains an escaped space.
        assert_eq!(
            strip_env_prefix("FOO=bar\\ baz git status"),
            Some(("FOO=bar\\ baz ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_single_quote_idiom() {
        // The '\'' idiom embeds a literal single quote: 'hello'\''world' = hello'world.
        // After fixing the regex to handle backslash-escape fragments, this should
        // parse the whole assignment as one token.
        assert_eq!(
            strip_env_prefix("FOO='hello'\\''world' cargo test"),
            Some((
                "FOO='hello'\\''world' ".to_string(),
                "cargo test".to_string()
            ))
        );
    }

    #[test]
    fn env_prefix_backslash_in_double_quoted_value() {
        // Escaped backslash inside double-quoted value.
        assert_eq!(
            strip_env_prefix(r#"FOO="bar\"baz" git status"#),
            Some((r#"FOO="bar\"baz" "#.to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_dollar_var_in_value() {
        // Shell variable expansions ($HOME, ${HOME}) are just non-whitespace chars.
        assert_eq!(
            strip_env_prefix("PREFIX=$HOME/bin git status"),
            Some(("PREFIX=$HOME/bin ".to_string(), "git status".to_string()))
        );
    }

    #[test]
    fn env_prefix_numeric_value() {
        assert_eq!(
            strip_env_prefix("DEBUG=123456 cargo test"),
            Some(("DEBUG=123456 ".to_string(), "cargo test".to_string()))
        );
    }

    #[test]
    fn env_prefix_numeric_key_not_matched() {
        // POSIX: variable names must start with a letter or underscore.
        assert_eq!(strip_env_prefix("1FOO=bar git status"), None);
    }
}
