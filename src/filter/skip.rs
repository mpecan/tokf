use regex::Regex;

/// Remove lines matching any of the given patterns.
///
/// Invalid regex patterns are silently dropped. An empty patterns list
/// returns all lines unchanged (passthrough).
pub fn apply_skip<'a>(patterns: &[String], lines: &[&'a str]) -> Vec<&'a str> {
    if patterns.is_empty() {
        return lines.to_vec();
    }

    let compiled: Vec<Regex> = patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();

    if compiled.is_empty() {
        return lines.to_vec();
    }

    lines
        .iter()
        .filter(|line| !compiled.iter().any(|re| re.is_match(line)))
        .copied()
        .collect()
}

/// Retain only lines matching at least one of the given patterns.
///
/// Invalid regex patterns are silently dropped. An empty patterns list
/// returns all lines unchanged (passthrough).
pub fn apply_keep<'a>(patterns: &[String], lines: &[&'a str]) -> Vec<&'a str> {
    if patterns.is_empty() {
        return lines.to_vec();
    }

    let compiled: Vec<Regex> = patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();

    if compiled.is_empty() {
        return lines.to_vec();
    }

    lines
        .iter()
        .filter(|line| compiled.iter().any(|re| re.is_match(line)))
        .copied()
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn skip_removes_matching_lines() {
        let patterns = vec!["^Enumerating".to_string(), "^Counting".to_string()];
        let lines = vec![
            "Enumerating objects: 5",
            "Counting objects: 100%",
            "abc1234..def5678 main -> main",
        ];
        let result = apply_skip(&patterns, &lines);
        assert_eq!(result, vec!["abc1234..def5678 main -> main"]);
    }

    #[test]
    fn skip_empty_patterns_passthrough() {
        let lines = vec!["a", "b", "c"];
        let result = apply_skip(&[], &lines);
        assert_eq!(result, lines);
    }

    #[test]
    fn skip_invalid_regex_dropped() {
        let patterns = vec!["[invalid".to_string(), "^b".to_string()];
        let lines = vec!["a", "b", "c"];
        let result = apply_skip(&patterns, &lines);
        assert_eq!(result, vec!["a", "c"]);
    }

    #[test]
    fn skip_all_invalid_regex_passthrough() {
        let patterns = vec!["[invalid".to_string()];
        let lines = vec!["a", "b"];
        let result = apply_skip(&patterns, &lines);
        assert_eq!(result, lines);
    }

    #[test]
    fn skip_no_matches_returns_all() {
        let patterns = vec!["^zzz".to_string()];
        let lines = vec!["a", "b"];
        let result = apply_skip(&patterns, &lines);
        assert_eq!(result, lines);
    }

    #[test]
    fn keep_retains_only_matching_lines() {
        let patterns = vec!["->".to_string()];
        let lines = vec!["Enumerating objects: 5", "abc1234..def5678 main -> main"];
        let result = apply_keep(&patterns, &lines);
        assert_eq!(result, vec!["abc1234..def5678 main -> main"]);
    }

    #[test]
    fn keep_empty_patterns_passthrough() {
        let lines = vec!["a", "b", "c"];
        let result = apply_keep(&[], &lines);
        assert_eq!(result, lines);
    }

    #[test]
    fn keep_invalid_regex_dropped() {
        let patterns = vec!["[invalid".to_string(), "^a".to_string()];
        let lines = vec!["a", "b", "c"];
        let result = apply_keep(&patterns, &lines);
        assert_eq!(result, vec!["a"]);
    }

    #[test]
    fn keep_all_invalid_regex_passthrough() {
        let patterns = vec!["[invalid".to_string()];
        let lines = vec!["a", "b"];
        let result = apply_keep(&patterns, &lines);
        assert_eq!(result, lines);
    }

    #[test]
    fn keep_no_matches_returns_empty() {
        let patterns = vec!["^zzz".to_string()];
        let lines = vec!["a", "b"];
        let result = apply_keep(&patterns, &lines);
        assert!(result.is_empty());
    }

    #[test]
    fn skip_multiple_patterns_all_applied() {
        let patterns = vec!["^a".to_string(), "^b".to_string(), "^c".to_string()];
        let lines = vec!["a1", "b2", "c3", "d4"];
        let result = apply_skip(&patterns, &lines);
        assert_eq!(result, vec!["d4"]);
    }
}
