use std::collections::HashMap;

use regex::Regex;

use crate::config::types::{OutputConfig, ParseConfig};

use super::extract::interpolate;
use super::group::{self, GroupCount};

/// Result of running the parse pipeline.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub vars: HashMap<String, String>,
    pub group_counts: Vec<GroupCount>,
}

/// Extract named variables from specific lines and collect groups
/// from the remaining lines.
pub fn run_parse(config: &ParseConfig, lines: &[&str]) -> ParseResult {
    let mut vars = HashMap::new();

    if let Some(ref branch_cfg) = config.branch
        && let Some(line) = lines.get(branch_cfg.line.saturating_sub(1))
        && let Ok(re) = Regex::new(&branch_cfg.pattern)
        && let Some(caps) = re.captures(line)
    {
        let value = interpolate(&branch_cfg.output, &caps);
        vars.insert("branch".to_string(), value);
    }

    let group_counts = config.group.as_ref().map_or_else(Vec::new, |group_cfg| {
        // Skip the branch line (line 1) for grouping — use remaining lines
        let start = config
            .branch
            .as_ref()
            .map_or(0, |b| b.line.min(lines.len()));
        group::collect_groups(group_cfg, &lines[start..])
    });

    ParseResult { vars, group_counts }
}

/// Render the final output by substituting named variables and
/// the rendered group counts into the format template.
pub fn render_output(output_config: &OutputConfig, parse_result: &ParseResult) -> String {
    let format_str = output_config
        .format
        .as_deref()
        .unwrap_or("{branch}\n{group_counts}");

    // Render group counts
    let group_counts_str = group::render_group_counts(
        &parse_result.group_counts,
        output_config
            .group_counts_format
            .as_deref()
            .unwrap_or("  {label}: {count}"),
        output_config.empty.as_deref(),
    );

    let mut result = format_str.to_string();

    // Substitute named vars
    for (key, value) in &parse_result.vars {
        result = result.replace(&format!("{{{key}}}"), value);
    }

    // Substitute {group_counts}
    result = result.replace("{group_counts}", &group_counts_str);

    // Clean up unresolved {name} placeholders → empty string
    if let Ok(cleanup) = Regex::new(r"\{[a-z_]+\}") {
        result = cleanup.replace_all(&result, "").to_string();
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::types::{ExtractRule, GroupConfig, LineExtract};

    fn git_status_parse_config() -> ParseConfig {
        let mut labels = BTreeMap::new();
        labels.insert("M ".to_string(), "modified".to_string());
        labels.insert(" M".to_string(), "modified (unstaged)".to_string());
        labels.insert("??".to_string(), "untracked".to_string());
        labels.insert("A ".to_string(), "added".to_string());
        labels.insert("D ".to_string(), "deleted".to_string());

        ParseConfig {
            branch: Some(LineExtract {
                line: 1,
                pattern: r"## (\S+?)(?:\.\.\.(\S+))?(?:\s+\[(.+)\])?$".to_string(),
                output: "{1}".to_string(),
            }),
            group: Some(GroupConfig {
                key: ExtractRule {
                    pattern: r"^(.{2}) ".to_string(),
                    output: "{1}".to_string(),
                },
                labels,
            }),
        }
    }

    fn git_status_output_config() -> OutputConfig {
        OutputConfig {
            format: Some("{branch}{tracking_info}\n{group_counts}".to_string()),
            group_counts_format: Some("  {label}: {count}".to_string()),
            empty: Some("clean \u{2014} nothing to commit".to_string()),
        }
    }

    #[test]
    fn run_parse_extracts_branch() {
        let config = git_status_parse_config();
        let lines = vec!["## main...origin/main", "M  src/main.rs"];
        let result = run_parse(&config, &lines);

        assert_eq!(result.vars.get("branch").unwrap(), "main");
    }

    #[test]
    fn run_parse_collects_groups() {
        let config = git_status_parse_config();
        let lines = vec![
            "## main...origin/main",
            "M  src/main.rs",
            "?? new.txt",
            "?? other.txt",
        ];
        let result = run_parse(&config, &lines);

        assert_eq!(result.group_counts.len(), 2);
        assert_eq!(result.group_counts[0].label, "modified");
        assert_eq!(result.group_counts[0].count, 1);
        assert_eq!(result.group_counts[1].label, "untracked");
        assert_eq!(result.group_counts[1].count, 2);
    }

    #[test]
    fn run_parse_no_branch_config() {
        let config = ParseConfig {
            branch: None,
            group: git_status_parse_config().group,
        };
        let lines = vec!["M  src/main.rs", "?? new.txt"];
        let result = run_parse(&config, &lines);

        assert!(result.vars.is_empty());
        assert_eq!(result.group_counts.len(), 2);
    }

    #[test]
    fn run_parse_empty_lines() {
        let config = git_status_parse_config();
        let result = run_parse(&config, &[]);

        assert!(result.vars.is_empty());
        assert!(result.group_counts.is_empty());
    }

    #[test]
    fn run_parse_branch_line_out_of_bounds() {
        let config = ParseConfig {
            branch: Some(LineExtract {
                line: 99,
                pattern: r"## (\S+)".to_string(),
                output: "{1}".to_string(),
            }),
            group: None,
        };
        let lines = vec!["only one line"];
        let result = run_parse(&config, &lines);

        assert!(result.vars.is_empty());
    }

    #[test]
    fn run_parse_invalid_branch_regex() {
        let config = ParseConfig {
            branch: Some(LineExtract {
                line: 1,
                pattern: "[invalid".to_string(),
                output: "{1}".to_string(),
            }),
            group: None,
        };
        let lines = vec!["## main...origin/main"];
        let result = run_parse(&config, &lines);

        assert!(result.vars.is_empty());
    }

    #[test]
    fn render_output_normal() {
        let config = git_status_parse_config();
        let output_config = git_status_output_config();
        let lines = vec![
            "## main...origin/main",
            "M  src/main.rs",
            " M src/lib.rs",
            "?? new.txt",
            "?? other.txt",
        ];
        let parse_result = run_parse(&config, &lines);
        let rendered = render_output(&output_config, &parse_result);

        assert_eq!(
            rendered,
            "main\n  modified: 1\n  modified (unstaged): 1\n  untracked: 2"
        );
    }

    #[test]
    fn render_output_clean_repo() {
        let config = git_status_parse_config();
        let output_config = git_status_output_config();
        let lines = vec!["## main...origin/main"];
        let parse_result = run_parse(&config, &lines);
        let rendered = render_output(&output_config, &parse_result);

        assert_eq!(rendered, "main\nclean \u{2014} nothing to commit");
    }

    #[test]
    fn render_output_default_config() {
        let output_config = OutputConfig::default();
        let parse_result = ParseResult {
            vars: HashMap::from([("branch".to_string(), "main".to_string())]),
            group_counts: vec![GroupCount {
                label: "modified".to_string(),
                count: 3,
            }],
        };
        let rendered = render_output(&output_config, &parse_result);

        assert_eq!(rendered, "main\n  modified: 3");
    }

    #[test]
    fn render_output_unresolved_vars_cleaned() {
        let output_config = git_status_output_config();
        let parse_result = ParseResult {
            vars: HashMap::from([("branch".to_string(), "main".to_string())]),
            group_counts: vec![],
        };
        let rendered = render_output(&output_config, &parse_result);

        // {tracking_info} should be cleaned to empty string
        assert_eq!(rendered, "main\nclean \u{2014} nothing to commit");
    }
}
