use std::collections::HashMap;

use regex::Regex;

use crate::config::types::GroupConfig;

use super::extract::interpolate;

/// A label with its occurrence count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCount {
    pub label: String,
    pub count: usize,
}

/// Compile the key pattern, extract a key per line via `interpolate()`,
/// map keys to labels (raw key as fallback), count per label,
/// and return results sorted alphabetically by label.
pub fn collect_groups(config: &GroupConfig, lines: &[&str]) -> Vec<GroupCount> {
    let Ok(re) = Regex::new(&config.key.pattern) else {
        return Vec::new();
    };

    let mut counts: HashMap<String, usize> = HashMap::new();

    for line in lines {
        if let Some(caps) = re.captures(line) {
            let raw_key = interpolate(&config.key.output, &caps);
            let label = config
                .labels
                .get(&raw_key)
                .cloned()
                .unwrap_or_else(|| raw_key.clone());
            *counts.entry(label).or_insert(0) += 1;
        }
    }

    let mut result: Vec<GroupCount> = counts
        .into_iter()
        .map(|(label, count)| GroupCount { label, count })
        .collect();
    result.sort_by(|a, b| a.label.cmp(&b.label));
    result
}

/// Render group counts using the given format template.
///
/// If `counts` is empty, returns `empty_text` (or empty string if `None`).
/// Otherwise applies `{label}` and `{count}` substitution to each group
/// and joins with newlines.
pub fn render_group_counts(counts: &[GroupCount], format: &str, empty: Option<&str>) -> String {
    if counts.is_empty() {
        return empty.unwrap_or("").to_string();
    }

    counts
        .iter()
        .map(|gc| {
            format
                .replace("{label}", &gc.label)
                .replace("{count}", &gc.count.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::config::types::ExtractRule;

    fn git_status_group_config() -> GroupConfig {
        let mut labels = BTreeMap::new();
        labels.insert("M ".to_string(), "modified".to_string());
        labels.insert(" M".to_string(), "modified (unstaged)".to_string());
        labels.insert("??".to_string(), "untracked".to_string());
        labels.insert("A ".to_string(), "added".to_string());
        labels.insert("D ".to_string(), "deleted".to_string());

        GroupConfig {
            key: ExtractRule {
                pattern: r"^(.{2}) ".to_string(),
                output: "{1}".to_string(),
            },
            labels,
        }
    }

    #[test]
    fn collect_groups_basic() {
        let config = git_status_group_config();
        let lines = vec![
            "M  src/main.rs",
            " M src/lib.rs",
            "?? new_file.txt",
            "?? another.txt",
        ];
        let groups = collect_groups(&config, &lines);

        assert_eq!(groups.len(), 3);
        // Alphabetical: modified, modified (unstaged), untracked
        assert_eq!(groups[0].label, "modified");
        assert_eq!(groups[0].count, 1);
        assert_eq!(groups[1].label, "modified (unstaged)");
        assert_eq!(groups[1].count, 1);
        assert_eq!(groups[2].label, "untracked");
        assert_eq!(groups[2].count, 2);
    }

    #[test]
    fn collect_groups_empty_lines() {
        let config = git_status_group_config();
        let groups = collect_groups(&config, &[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn collect_groups_no_matches() {
        let config = git_status_group_config();
        // Lines that don't match ^(.{2}) — too short or no space at position 3
        let lines = vec!["x", "ab"];
        let groups = collect_groups(&config, &lines);
        assert!(groups.is_empty());
    }

    #[test]
    fn collect_groups_invalid_regex() {
        let config = GroupConfig {
            key: ExtractRule {
                pattern: "[invalid".to_string(),
                output: "{1}".to_string(),
            },
            labels: BTreeMap::new(),
        };
        let lines = vec!["M  src/main.rs"];
        let groups = collect_groups(&config, &lines);
        assert!(groups.is_empty());
    }

    #[test]
    fn collect_groups_unknown_key_uses_raw() {
        let config = GroupConfig {
            key: ExtractRule {
                pattern: r"^(.{2}) ".to_string(),
                output: "{1}".to_string(),
            },
            labels: BTreeMap::new(),
        };
        let lines = vec!["M  src/main.rs", "M  src/lib.rs"];
        let groups = collect_groups(&config, &lines);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "M ");
        assert_eq!(groups[0].count, 2);
    }

    #[test]
    fn render_group_counts_basic() {
        let counts = vec![
            GroupCount {
                label: "modified".to_string(),
                count: 2,
            },
            GroupCount {
                label: "untracked".to_string(),
                count: 1,
            },
        ];
        let result = render_group_counts(&counts, "  {label}: {count}", None);
        assert_eq!(result, "  modified: 2\n  untracked: 1");
    }

    #[test]
    fn render_group_counts_empty_with_message() {
        let result = render_group_counts(
            &[],
            "  {label}: {count}",
            Some("clean \u{2014} nothing to commit"),
        );
        assert_eq!(result, "clean \u{2014} nothing to commit");
    }

    #[test]
    fn render_group_counts_empty_no_message() {
        let result = render_group_counts(&[], "  {label}: {count}", None);
        assert_eq!(result, "");
    }

    #[test]
    fn render_group_counts_single() {
        let counts = vec![GroupCount {
            label: "added".to_string(),
            count: 5,
        }];
        let result = render_group_counts(&counts, "{label} ({count})", None);
        assert_eq!(result, "added (5)");
    }
}
