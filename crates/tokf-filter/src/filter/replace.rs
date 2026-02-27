use regex::Regex;

use tokf_common::config::types::ReplaceRule;

/// Apply `[[replace]]` rules to each line, in order.
///
/// Rules run sequentially: each rule's output becomes the next rule's input.
/// When a rule's pattern matches, the line is replaced via capture interpolation.
/// When it does not match, the line passes through unchanged.
/// Invalid regex patterns are silently skipped.
pub fn apply_replace(rules: &[ReplaceRule], lines: &[&str]) -> Vec<String> {
    // Compile all regexes up front, pairing each rule with its compiled regex.
    // Rules with invalid patterns are silently dropped.
    let compiled: Vec<(Regex, &str)> = rules
        .iter()
        .filter_map(|r| {
            Regex::new(&r.pattern)
                .ok()
                .map(|re| (re, r.output.as_str()))
        })
        .collect();

    lines
        .iter()
        .map(|line| apply_rules_to_line(&compiled, line))
        .collect()
}

fn apply_rules_to_line(compiled: &[(Regex, &str)], line: &str) -> String {
    let mut current = line.to_string();
    for (re, output_tmpl) in compiled {
        if let Some(caps) = re.captures(&current) {
            current = super::extract::interpolate(output_tmpl, &caps);
        }
    }
    current
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn rule(pattern: &str, output: &str) -> ReplaceRule {
        ReplaceRule {
            pattern: pattern.to_string(),
            output: output.to_string(),
        }
    }

    #[test]
    fn replace_no_rules_passthrough() {
        let lines = vec!["hello", "world"];
        let result = apply_replace(&[], &lines);
        assert_eq!(result, vec!["hello".to_string(), "world".to_string()]);
    }

    #[test]
    fn replace_single_rule_matches() {
        let rules = vec![rule(r"^(\S+)\s+(\S+)\s+(\S+)", "{1}: {2} \u{2192} {3}")];
        let lines = vec!["pkg  1.0  2.0"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["pkg: 1.0 \u{2192} 2.0".to_string()]);
    }

    #[test]
    fn replace_no_match_passthrough() {
        let rules = vec![rule(r"NOMATCH", "replaced")];
        let lines = vec!["hello world"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["hello world".to_string()]);
    }

    #[test]
    fn replace_multiple_rules_chain() {
        // Rule 1: "foo" → "bar"; Rule 2: "bar" → "baz"
        let rules = vec![rule(r"foo", "bar"), rule(r"bar", "baz")];
        let lines = vec!["foo"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["baz".to_string()]);
    }

    #[test]
    fn replace_invalid_regex_skipped() {
        let rules = vec![rule(r"[invalid", "never"), rule(r"hello", "world")];
        let lines = vec!["hello"];
        let result = apply_replace(&rules, &lines);
        // invalid regex is skipped; second rule applies
        assert_eq!(result, vec!["world".to_string()]);
    }

    #[test]
    fn replace_empty_input_returns_empty() {
        let rules = vec![rule(r"x", "y")];
        let result = apply_replace(&rules, &[]);
        assert!(result.is_empty());
    }
}
