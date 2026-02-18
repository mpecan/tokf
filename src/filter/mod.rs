mod extract;
mod group;
mod parse;
mod skip;

use crate::config::types::{FilterConfig, MatchOutputRule, OutputBranch};
use crate::runner::CommandResult;

/// The result of applying a filter to command output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterResult {
    pub output: String,
}

/// Apply a filter configuration to a command result.
///
/// Processing order:
/// 1. `match_output` — substring check against combined output, first match wins
/// 2. Top-level `skip`/`keep` pre-filtering
/// 3. If `parse` exists → parse+output (alternative path to branches)
/// 4. Select branch by exit code (0 → `on_success`, else → `on_failure`)
/// 5. Apply the selected branch's rules or passthrough
pub fn apply(config: &FilterConfig, result: &CommandResult) -> FilterResult {
    // 1. match_output short-circuit
    if let Some(rule) = find_matching_rule(&config.match_output, &result.combined) {
        return FilterResult {
            output: rule.output.clone(),
        };
    }

    // 2. Top-level skip/keep pre-filtering
    let lines: Vec<&str> = result.combined.lines().collect();
    let lines = skip::apply_skip(&config.skip, &lines);
    let lines = skip::apply_keep(&config.keep, &lines);

    // 3. If parse exists → parse+output pipeline
    if let Some(ref parse_config) = config.parse {
        let parse_result = parse::run_parse(parse_config, &lines);
        let output_config = config.output.clone().unwrap_or_default();
        let output = parse::render_output(&output_config, &parse_result);
        return FilterResult { output };
    }

    // 4. Select branch by exit code
    let branch = select_branch(config, result.exit_code);

    // 5. Apply branch or passthrough (on pre-filtered lines)
    let pre_filtered = lines.join("\n");
    let output = branch.map_or_else(|| pre_filtered.clone(), |b| apply_branch(b, &pre_filtered));

    FilterResult { output }
}

/// Find the first `match_output` rule whose `contains` substring appears
/// in the combined output. Returns the matching rule, or `None`.
fn find_matching_rule<'a>(
    rules: &'a [MatchOutputRule],
    combined: &str,
) -> Option<&'a MatchOutputRule> {
    rules.iter().find(|rule| combined.contains(&rule.contains))
}

/// Select the output branch based on exit code.
/// Exit code 0 → `on_success`, anything else → `on_failure`.
const fn select_branch(config: &FilterConfig, exit_code: i32) -> Option<&OutputBranch> {
    if exit_code == 0 {
        config.on_success.as_ref()
    } else {
        config.on_failure.as_ref()
    }
}

/// Apply a branch's processing rules to the combined output.
///
/// Processing order:
/// 1. Fixed `output` string → return immediately
/// 2. `tail` / `head` truncation
/// 3. `skip` patterns
/// 4. `extract` rule
/// 5. Remaining lines joined with `\n`
fn apply_branch(branch: &OutputBranch, combined: &str) -> String {
    // 1. Fixed output — short-circuit
    if let Some(ref output) = branch.output {
        return output.clone();
    }

    let mut lines: Vec<&str> = combined.lines().collect();

    // 2. tail / head truncation
    if let Some(tail) = branch.tail
        && lines.len() > tail
    {
        lines = lines.split_off(lines.len() - tail);
    }
    if let Some(head) = branch.head {
        lines.truncate(head);
    }

    // 3. skip patterns
    lines = skip::apply_skip(&branch.skip, &lines);

    // 4. extract rule
    if let Some(ref rule) = branch.extract {
        return extract::apply_extract(rule, &lines);
    }

    // 5. Join remaining lines
    lines.join("\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::config::types::ExtractRule;

    fn make_result(combined: &str, exit_code: i32) -> CommandResult {
        CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code,
            combined: combined.to_string(),
        }
    }

    fn minimal_config() -> FilterConfig {
        toml::from_str(r#"command = "test""#).unwrap()
    }

    // --- find_matching_rule ---

    #[test]
    fn match_output_first_match_wins() {
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
    fn match_output_no_match_returns_none() {
        let rules = vec![MatchOutputRule {
            contains: "NOMATCH".to_string(),
            output: "nope".to_string(),
        }];
        assert!(find_matching_rule(&rules, "some output").is_none());
    }

    #[test]
    fn match_output_empty_rules() {
        assert!(find_matching_rule(&[], "anything").is_none());
    }

    #[test]
    fn match_output_is_case_sensitive() {
        let rules = vec![MatchOutputRule {
            contains: "Fatal".to_string(),
            output: "found".to_string(),
        }];
        assert!(find_matching_rule(&rules, "fatal: error").is_none());
        assert!(find_matching_rule(&rules, "Fatal: error").is_some());
    }

    // --- select_branch ---

    #[test]
    fn select_branch_success() {
        let mut config = minimal_config();
        config.on_success = Some(OutputBranch {
            output: Some("success".to_string()),
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: None,
        });
        assert!(select_branch(&config, 0).is_some());
        assert!(select_branch(&config, 1).is_none());
    }

    #[test]
    fn select_branch_failure() {
        let mut config = minimal_config();
        config.on_failure = Some(OutputBranch {
            output: Some("failure".to_string()),
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: None,
        });
        assert!(select_branch(&config, 0).is_none());
        assert!(select_branch(&config, 1).is_some());
        assert!(select_branch(&config, 127).is_some());
    }

    // --- apply_branch ---

    #[test]
    fn branch_fixed_output() {
        let branch = OutputBranch {
            output: Some("ok \u{2713}".to_string()),
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "anything"), "ok \u{2713}");
    }

    #[test]
    fn branch_tail_truncation() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: Some(2),
            head: None,
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "a\nb\nc\nd"), "c\nd");
    }

    #[test]
    fn branch_head_truncation() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: Some(2),
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "a\nb\nc\nd"), "a\nb");
    }

    #[test]
    fn branch_tail_then_head() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: Some(3),
            head: Some(2),
            skip: vec![],
            extract: None,
        };
        // tail 3 of [a,b,c,d] → [b,c,d], then head 2 → [b,c]
        assert_eq!(apply_branch(&branch, "a\nb\nc\nd"), "b\nc");
    }

    #[test]
    fn branch_skip_then_join() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: None,
            skip: vec!["^noise".to_string()],
            extract: None,
        };
        assert_eq!(
            apply_branch(&branch, "noise line\nkeep me\nnoise again"),
            "keep me"
        );
    }

    #[test]
    fn branch_extract() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: Some(ExtractRule {
                pattern: r"(\S+)\s*->\s*(\S+)".to_string(),
                output: "ok {2}".to_string(),
            }),
        };
        assert_eq!(apply_branch(&branch, "main -> main"), "ok main");
    }

    #[test]
    fn branch_tail_less_than_lines() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: Some(10),
            head: None,
            skip: vec![],
            extract: None,
        };
        // Only 3 lines, tail 10 → all lines kept
        assert_eq!(apply_branch(&branch, "a\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn branch_empty_string_returns_empty() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, ""), "");
    }

    #[test]
    fn branch_single_line_no_newline() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: None,
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "only-line"), "only-line");
    }

    #[test]
    fn branch_tail_zero_returns_empty() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: Some(0),
            head: None,
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "a\nb\nc"), "");
    }

    #[test]
    fn branch_head_zero_returns_empty() {
        let branch = OutputBranch {
            output: None,
            aggregate: None,
            tail: None,
            head: Some(0),
            skip: vec![],
            extract: None,
        };
        assert_eq!(apply_branch(&branch, "a\nb\nc"), "");
    }

    // --- apply (full pipeline) ---

    #[test]
    fn apply_match_output_short_circuits() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
match_output = [
  { contains = "special", output = "found it" },
]

[on_success]
output = "should not reach"
"#,
        )
        .unwrap();

        let result = make_result("some special output", 0);
        assert_eq!(apply(&config, &result).output, "found it");
    }

    #[test]
    fn apply_passthrough_no_branch() {
        let config = minimal_config();
        let result = make_result("raw output", 0);
        assert_eq!(apply(&config, &result).output, "raw output");
    }

    #[test]
    fn apply_success_branch() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
[on_success]
output = "ok"
"#,
        )
        .unwrap();

        let result = make_result("anything", 0);
        assert_eq!(apply(&config, &result).output, "ok");
    }

    #[test]
    fn apply_failure_branch() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
[on_failure]
tail = 2
"#,
        )
        .unwrap();

        let result = make_result("a\nb\nc\nd", 1);
        assert_eq!(apply(&config, &result).output, "c\nd");
    }

    #[test]
    fn apply_full_skip_then_extract() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"

[on_success]
skip = ["^noise"]
extract = { pattern = '(\w+) -> (\w+)', output = "pushed {2}" }
"#,
        )
        .unwrap();

        let result = make_result("noise line\nmain -> main\nnoise again", 0);
        assert_eq!(apply(&config, &result).output, "pushed main");
    }

    // --- parse pipeline tests ---

    #[test]
    fn apply_parse_overrides_on_success() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }

[on_success]
output = "should not appear"
"#,
        )
        .unwrap();

        let result = make_result("## main", 0);
        assert_eq!(apply(&config, &result).output, "main\n");
    }

    #[test]
    fn apply_parse_overrides_on_failure() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }

[on_failure]
output = "should not appear"
"#,
        )
        .unwrap();

        let result = make_result("## develop", 1);
        assert_eq!(apply(&config, &result).output, "develop\n");
    }

    #[test]
    fn apply_match_output_overrides_parse() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
match_output = [
  { contains = "fatal", output = "error!" },
]

[parse]
branch = { line = 1, pattern = '## (\S+)', output = "{1}" }
"#,
        )
        .unwrap();

        let result = make_result("fatal: something broke", 128);
        assert_eq!(apply(&config, &result).output, "error!");
    }

    #[test]
    fn apply_top_level_skip_affects_parse() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
skip = ["^#"]

[parse]
branch = { line = 1, pattern = '^(\S+)', output = "{1}" }
"#,
        )
        .unwrap();

        // After skip removes "# comment", the first line becomes "M  file.rs"
        let result = make_result("# comment\nM  file.rs", 0);
        assert_eq!(apply(&config, &result).output, "M\n");
    }

    #[test]
    fn apply_top_level_keep_affects_branch_path() {
        let config: FilterConfig = toml::from_str(
            r#"
command = "test"
keep = ["^keep"]
"#,
        )
        .unwrap();

        let result = make_result("drop me\nkeep this\ndrop too\nkeep that", 0);
        assert_eq!(apply(&config, &result).output, "keep this\nkeep that");
    }
}
