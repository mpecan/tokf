use regex::Regex;

use crate::config::types::ExtractRule;

/// Apply an extract rule across lines — first match wins.
///
/// Returns the interpolated template on match. On invalid regex or no match,
/// returns all lines joined with newlines (passthrough).
pub fn apply_extract(rule: &ExtractRule, lines: &[&str]) -> String {
    let Ok(re) = Regex::new(&rule.pattern) else {
        return lines.join("\n");
    };

    for line in lines {
        if let Some(caps) = re.captures(line) {
            return interpolate(&rule.output, &caps);
        }
    }

    lines.join("\n")
}

/// Replace `{0}`, `{1}`, `{2}`, ... placeholders with capture groups.
///
/// Iterates in reverse order so `{10}` is replaced before `{1}`.
/// Missing groups become empty strings.
pub fn interpolate(template: &str, caps: &regex::Captures<'_>) -> String {
    let mut result = template.to_string();
    let max_group = caps.len().saturating_sub(1);

    for i in (0..=max_group).rev() {
        let placeholder = format!("{{{i}}}");
        let value = caps.get(i).map_or("", |m| m.as_str());
        result = result.replace(&placeholder, value);
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn rule(pattern: &str, output: &str) -> ExtractRule {
        ExtractRule {
            pattern: pattern.to_string(),
            output: output.to_string(),
        }
    }

    #[test]
    fn extract_first_match_wins() {
        let r = rule(r"(\S+)\s*->\s*(\S+)", "ok \u{2713} {2}");
        let lines = vec![
            "Enumerating objects: 5",
            "abc1234..def5678 main -> main",
            "another -> branch",
        ];
        assert_eq!(apply_extract(&r, &lines), "ok \u{2713} main");
    }

    #[test]
    fn extract_no_match_passthrough() {
        let r = rule(r"NOMATCH", "{1}");
        let lines = vec!["line one", "line two"];
        assert_eq!(apply_extract(&r, &lines), "line one\nline two");
    }

    #[test]
    fn extract_invalid_regex_passthrough() {
        let r = rule(r"[invalid", "{1}");
        let lines = vec!["line one", "line two"];
        assert_eq!(apply_extract(&r, &lines), "line one\nline two");
    }

    #[test]
    fn extract_empty_lines_no_match() {
        let r = rule(r"(\d+)", "{1}");
        let lines: Vec<&str> = vec![];
        assert_eq!(apply_extract(&r, &lines), "");
    }

    #[test]
    fn interpolate_replaces_numbered_groups() {
        let re = Regex::new(r"^\[(\S+)\s+(\w+)\]").unwrap();
        let caps = re.captures("[main abc1234] Add feature X").unwrap();
        assert_eq!(interpolate("ok \u{2713} {2}", &caps), "ok \u{2713} abc1234");
    }

    #[test]
    fn interpolate_group_zero_is_full_match() {
        let re = Regex::new(r"(hello) (world)").unwrap();
        let caps = re.captures("hello world").unwrap();
        assert_eq!(interpolate("{0}", &caps), "hello world");
    }

    #[test]
    fn interpolate_missing_group_becomes_empty() {
        let re = Regex::new(r"(a)(b)?").unwrap();
        let caps = re.captures("a").unwrap();
        assert_eq!(interpolate("{1}-{2}", &caps), "a-");
    }

    #[test]
    fn interpolate_reverse_order_prevents_partial_replace() {
        // Ensure {10} doesn't get mangled by {1} replacement first
        let re = Regex::new(r"(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)").unwrap();
        let caps = re.captures("abcdefghijk").unwrap();
        assert_eq!(interpolate("{10}", &caps), "j");
    }

    #[test]
    fn extract_git_commit_pattern() {
        let r = rule(r"^\[(\S+)\s+(\w+)\]", "ok \u{2713} {2}");
        let lines = vec![
            "[main abc1234] Add feature X",
            " 1 file changed, 10 insertions(+), 2 deletions(-)",
        ];
        assert_eq!(apply_extract(&r, &lines), "ok \u{2713} abc1234");
    }

    #[test]
    fn extract_git_push_pattern() {
        let r = rule(r"(\S+)\s*->\s*(\S+)", "ok \u{2713} {2}");
        let lines = vec!["   abc1234..def5678 main -> main"];
        assert_eq!(apply_extract(&r, &lines), "ok \u{2713} main");
    }
}
