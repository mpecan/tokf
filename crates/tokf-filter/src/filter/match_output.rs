use std::collections::HashMap;

use tokf_common::config::types::MatchOutputRule;

use super::section::SectionMap;
use super::template;

/// Find the first `match_output` rule whose `contains` substring appears
/// in the combined output. Returns the matching rule, or `None`.
pub fn find_matching_rule<'a>(
    rules: &'a [MatchOutputRule],
    combined: &str,
) -> Option<&'a MatchOutputRule> {
    rules.iter().find(|rule| combined.contains(&rule.contains))
}

/// Render a `match_output` rule's output template, resolving `{line_containing}`
/// to the first line that contains the matched substring, and `{output}` to the
/// full combined output.
pub fn render_output(output_tmpl: &str, contains: &str, combined: &str) -> String {
    let mut vars = HashMap::new();
    if let Some(line) = combined.lines().find(|l| l.contains(contains)) {
        vars.insert("line_containing".to_string(), line.to_string());
    }
    vars.insert("output".to_string(), combined.to_string());
    template::render_template(output_tmpl, &vars, &SectionMap::new())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::literal_string_with_formatting_args
)]
mod tests {
    use super::*;

    // --- find_matching_rule ---

    #[test]
    fn first_match_wins() {
        let rules = vec![
            MatchOutputRule {
                contains: "up-to-date".to_string(),
                output: "ok (up-to-date)".to_string(),
            },
            MatchOutputRule {
                contains: "rejected".to_string(),
                output: "rejected!".to_string(),
            },
        ];
        let matched = find_matching_rule(&rules, "Everything up-to-date");
        assert_eq!(matched.unwrap().output, "ok (up-to-date)");
    }

    #[test]
    fn no_match_returns_none() {
        let rules = vec![MatchOutputRule {
            contains: "NOMATCH".to_string(),
            output: "nope".to_string(),
        }];
        assert!(find_matching_rule(&rules, "some output").is_none());
    }

    #[test]
    fn empty_rules() {
        assert!(find_matching_rule(&[], "anything").is_none());
    }

    #[test]
    fn case_sensitive() {
        let rules = vec![MatchOutputRule {
            contains: "Fatal".to_string(),
            output: "found".to_string(),
        }];
        assert!(find_matching_rule(&rules, "fatal: error").is_none());
        assert!(find_matching_rule(&rules, "Fatal: error").is_some());
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
}
