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
}
