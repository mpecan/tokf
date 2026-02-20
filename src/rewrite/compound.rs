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
    let bytes = command.as_bytes();
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
                i += 1; // skip the escaped character
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
                        return true;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    false
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
}
