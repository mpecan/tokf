use regex::Regex;

use tokf_common::config::types::ReplaceRule;

/// A compiled replace rule, ready to apply.
struct CompiledRule<'a> {
    re: Regex,
    output: &'a str,
    replace_all: bool,
}

/// Apply `[[replace]]` rules to each line, in order.
///
/// Rules run sequentially: each rule's output becomes the next rule's input.
/// When a rule's pattern matches, the line is replaced via capture interpolation.
/// When it does not match, the line passes through unchanged.
/// Invalid regex patterns are silently skipped.
///
/// When `replace_all` is true on a rule, all non-overlapping matches in each
/// line are replaced in-place (like `Regex::replace_all`), preserving unmatched
/// portions of the line.
pub fn apply_replace(rules: &[ReplaceRule], lines: &[&str]) -> Vec<String> {
    // Compile all regexes up front. Rules with invalid patterns are silently dropped.
    let compiled: Vec<CompiledRule<'_>> = rules
        .iter()
        .filter_map(|r| {
            Regex::new(&r.pattern).ok().map(|re| CompiledRule {
                re,
                output: r.output.as_str(),
                replace_all: r.replace_all,
            })
        })
        .collect();

    lines
        .iter()
        .map(|line| apply_rules_to_line(&compiled, line))
        .collect()
}

fn apply_rules_to_line(compiled: &[CompiledRule<'_>], line: &str) -> String {
    let mut current = line.to_string();
    for rule in compiled {
        if rule.replace_all {
            // Replace all non-overlapping matches in-place, preserving unmatched text.
            // Uses a closure to call interpolate per match so both {N} and $N work.
            let replaced = rule.re.replace_all(&current, |caps: &regex::Captures| {
                super::extract::interpolate(rule.output, caps)
            });
            // Only allocate when replace_all actually changed something.
            if let std::borrow::Cow::Owned(s) = replaced {
                current = s;
            }
        } else if let Some(caps) = rule.re.captures(&current) {
            current = super::extract::interpolate(rule.output, &caps);
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
            replace_all: false,
        }
    }

    fn rule_all(pattern: &str, output: &str) -> ReplaceRule {
        ReplaceRule {
            pattern: pattern.to_string(),
            output: output.to_string(),
            replace_all: true,
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

    // --- replace_all tests ---

    #[test]
    fn replace_all_replaces_every_occurrence() {
        let rules = vec![rule_all(r"foo", "bar")];
        let lines = vec!["foo baz foo"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["bar baz bar".to_string()]);
    }

    #[test]
    fn replace_all_preserves_unmatched_text() {
        let rules = vec![rule_all(r"\d+", "N")];
        let lines = vec!["line 42 has 3 numbers"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["line N has N numbers".to_string()]);
    }

    #[test]
    fn replace_all_no_match_passthrough() {
        let rules = vec![rule_all(r"NOMATCH", "replaced")];
        let lines = vec!["hello world"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["hello world".to_string()]);
    }

    #[test]
    fn replace_all_with_backreferences() {
        let rules = vec![rule_all(r"(\w+):(\w+)", "$2:$1")];
        let lines = vec!["a:b c:d"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["b:a d:c".to_string()]);
    }

    #[test]
    fn replace_all_chaining_sequential() {
        let rules = vec![rule_all(r"aaa", "bbb"), rule_all(r"bbb", "ccc")];
        let lines = vec!["aaa xxx aaa"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["ccc xxx ccc".to_string()]);
    }

    #[test]
    fn replace_all_multiple_lines() {
        let rules = vec![rule_all(r"foo", "bar")];
        let lines = vec!["foo baz foo", "foo"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["bar baz bar".to_string(), "bar".to_string()]);
    }

    #[test]
    fn replace_all_with_tokf_native_braces_syntax() {
        // {N} syntax must work in replace_all mode (via interpolate)
        let rules = vec![rule_all(r"(\w+):(\w+)", "{2}:{1}")];
        let lines = vec!["a:b c:d"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["b:a d:c".to_string()]);
    }

    #[test]
    fn replace_all_with_mixed_syntax() {
        // Mixed {N} and $N in same template
        let rules = vec![rule_all(r"(\w+):(\w+)", "{1}=$2")];
        let lines = vec!["x:y z:w"];
        let result = apply_replace(&rules, &lines);
        assert_eq!(result, vec!["x=y z=w".to_string()]);
    }
}
