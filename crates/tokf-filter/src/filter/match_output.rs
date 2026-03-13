use std::collections::HashMap;

use regex::Regex;
use tokf_common::config::types::MatchOutputRule;

use super::section::SectionMap;
use super::template;

/// Find the first `match_output` rule that matches the combined output.
///
/// Matching order per rule:
/// 1. If `unless` is set and matches → skip this rule
/// 2. If `contains` is set and found as substring → match
/// 3. If `pattern` is set and matches as regex → match
///
/// Returns the matching rule and the matched substring (for `{line_containing}`
/// template resolution), or `None`.
pub fn find_matching_rule<'a>(
    rules: &'a [MatchOutputRule],
    combined: &str,
) -> Option<(&'a MatchOutputRule, String)> {
    for rule in rules {
        // Check `unless` guard first — if this regex matches, skip the rule.
        if let Some(ref unless_pat) = rule.unless
            && let Ok(re) = Regex::new(unless_pat)
            && re.is_match(combined)
        {
            continue;
        }

        // Try literal substring match first (`contains`).
        if let Some(ref needle) = rule.contains
            && combined.contains(needle.as_str())
        {
            return Some((rule, needle.clone()));
        }

        // Fall back to regex match (`pattern`).
        if let Some(ref pat) = rule.pattern
            && let Ok(re) = Regex::new(pat)
            && re.is_match(combined)
        {
            // Use the regex pattern itself as the "needle" for
            // `{line_containing}` — find the first line that matches.
            let needle = combined
                .lines()
                .find(|l| re.is_match(l))
                .unwrap_or("")
                .to_string();
            return Some((rule, needle));
        }
    }
    None
}

/// Render a `match_output` rule's output template, resolving `{line_containing}`
/// to the first line that contains the matched substring, and `{output}` to the
/// full combined output.
pub fn render_output(output_tmpl: &str, needle: &str, combined: &str) -> String {
    let mut vars = HashMap::new();
    if !needle.is_empty()
        && let Some(line) = combined.lines().find(|l| l.contains(needle))
    {
        vars.insert("line_containing".to_string(), line.to_string());
    }
    vars.insert("output".to_string(), combined.to_string());
    template::render_template(
        output_tmpl,
        &vars,
        &SectionMap::new(),
        &template::ChunkMap::new(),
    )
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::literal_string_with_formatting_args
)]
mod tests {
    use super::*;

    fn rule_contains(contains: &str, output: &str) -> MatchOutputRule {
        MatchOutputRule {
            contains: Some(contains.to_string()),
            pattern: None,
            output: output.to_string(),
            unless: None,
        }
    }

    fn rule_pattern(pattern: &str, output: &str) -> MatchOutputRule {
        MatchOutputRule {
            contains: None,
            pattern: Some(pattern.to_string()),
            output: output.to_string(),
            unless: None,
        }
    }

    // --- find_matching_rule (contains) ---

    #[test]
    fn first_match_wins() {
        let rules = vec![
            rule_contains("up-to-date", "ok (up-to-date)"),
            rule_contains("rejected", "rejected!"),
        ];
        let (matched, _) = find_matching_rule(&rules, "Everything up-to-date").unwrap();
        assert_eq!(matched.output, "ok (up-to-date)");
    }

    #[test]
    fn no_match_returns_none() {
        let rules = vec![rule_contains("NOMATCH", "nope")];
        assert!(find_matching_rule(&rules, "some output").is_none());
    }

    #[test]
    fn empty_rules() {
        assert!(find_matching_rule(&[], "anything").is_none());
    }

    #[test]
    fn case_sensitive() {
        let rules = vec![rule_contains("Fatal", "found")];
        assert!(find_matching_rule(&rules, "fatal: error").is_none());
        assert!(find_matching_rule(&rules, "Fatal: error").is_some());
    }

    // --- find_matching_rule (pattern — regex) ---

    #[test]
    fn pattern_regex_match() {
        let rules = vec![rule_pattern(
            r"0 Warning\(s\)\n\s+0 Error\(s\)",
            "ok (build succeeded)",
        )];
        let (matched, _) =
            find_matching_rule(&rules, "  0 Warning(s)\n  0 Error(s)\nDone").unwrap();
        assert_eq!(matched.output, "ok (build succeeded)");
    }

    #[test]
    fn pattern_regex_no_match() {
        let rules = vec![rule_pattern(r"^NOMATCH$", "nope")];
        assert!(find_matching_rule(&rules, "some output").is_none());
    }

    #[test]
    fn pattern_invalid_regex_skipped() {
        let rules = vec![
            MatchOutputRule {
                contains: None,
                pattern: Some("[invalid".to_string()),
                output: "bad".to_string(),
                unless: None,
            },
            rule_contains("fallback", "found"),
        ];
        let (matched, _) = find_matching_rule(&rules, "try fallback").unwrap();
        assert_eq!(matched.output, "found");
    }

    #[test]
    fn contains_takes_priority_over_pattern() {
        let rules = vec![MatchOutputRule {
            contains: Some("literal".to_string()),
            pattern: Some(r"regex".to_string()),
            output: "matched".to_string(),
            unless: None,
        }];
        // Has "literal" but not "regex" → should match via contains
        let result = find_matching_rule(&rules, "has literal text");
        assert!(result.is_some());
    }

    // --- unless guard ---

    #[test]
    fn unless_prevents_match() {
        let rules = vec![MatchOutputRule {
            contains: Some("total size is".to_string()),
            pattern: None,
            output: "ok (synced)".to_string(),
            unless: Some(r"error|failed".to_string()),
        }];
        // Has "total size is" AND "error" → unless fires, rule skipped
        assert!(find_matching_rule(&rules, "total size is 42\nerror: something failed").is_none());
    }

    #[test]
    fn unless_allows_match_when_no_error() {
        let rules = vec![MatchOutputRule {
            contains: Some("total size is".to_string()),
            pattern: None,
            output: "ok (synced)".to_string(),
            unless: Some(r"error|failed".to_string()),
        }];
        let (matched, _) = find_matching_rule(&rules, "total size is 42\nall good").unwrap();
        assert_eq!(matched.output, "ok (synced)");
    }

    #[test]
    fn unless_invalid_regex_ignored() {
        let rules = vec![MatchOutputRule {
            contains: Some("needle".to_string()),
            pattern: None,
            output: "found".to_string(),
            unless: Some("[invalid".to_string()),
        }];
        // Invalid unless regex → guard doesn't fire, rule matches normally
        let result = find_matching_rule(&rules, "has needle");
        assert!(result.is_some());
    }

    // --- render_output ---

    #[test]
    fn resolves_line_containing() {
        let output = render_output(
            "\u{2717} {line_containing}",
            "fatal:",
            "some preamble\nfatal: bad revision\nmore stuff",
        );
        assert_eq!(output, "\u{2717} fatal: bad revision");
    }

    #[test]
    fn resolves_output_var() {
        let output = render_output("matched: {output}", "keyword", "line with keyword");
        assert_eq!(output, "matched: line with keyword");
    }

    #[test]
    fn plain_string_passthrough() {
        let output = render_output("ok (up-to-date)", "up-to-date", "Everything up-to-date");
        assert_eq!(output, "ok (up-to-date)");
    }

    #[test]
    fn no_matching_line_empty_var() {
        let output = render_output("\u{2717} {line_containing}", "fatal:", "no match here");
        // "fatal:" not found in any line → {line_containing} resolves to ""
        assert_eq!(output, "\u{2717} ");
    }

    // --- TOML deserialization ---

    #[test]
    fn toml_contains_style() {
        let rule: MatchOutputRule =
            toml::from_str(r#"contains = "error"\noutput = "bad""#.replace(r"\n", "\n").as_str())
                .unwrap();
        assert_eq!(rule.contains.unwrap(), "error");
        assert!(rule.pattern.is_none());
        assert!(rule.unless.is_none());
    }

    #[test]
    fn toml_pattern_style() {
        let rule: MatchOutputRule = toml::from_str(
            r#"pattern = "0 Error\\(s\\)"\nmessage = "ok""#.replace(r"\n", "\n").as_str(),
        )
        .unwrap();
        assert!(rule.contains.is_none());
        assert_eq!(rule.pattern.unwrap(), r"0 Error\(s\)");
        assert_eq!(rule.output, "ok"); // "message" aliased to "output"
    }

    #[test]
    fn toml_unless_style() {
        let rule: MatchOutputRule = toml::from_str(
            r#"
pattern = "total size is"
output = "ok"
unless = "error|failed"
"#,
        )
        .unwrap();
        assert_eq!(rule.unless.unwrap(), "error|failed");
    }
}
