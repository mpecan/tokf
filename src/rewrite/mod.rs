pub mod types;

pub(crate) mod compound;
pub(crate) mod rules;
pub(crate) mod user_config;

use std::path::PathBuf;

use crate::config;
use compound::split_compound;
use rules::{apply_rules, should_skip};
use types::{RewriteConfig, RewriteRule};

pub use user_config::load_user_config;

/// Build rewrite rules by discovering installed filters (recursive walk).
///
/// For each filter pattern, generates a rule:
/// `^{command_pattern}(\s.*)?$` → `tokf run {0}`
///
/// Handles `CommandPattern::Multiple` (one rule per pattern string) and
/// wildcards (`*` → `\S+` in the regex).
pub(crate) fn build_rules_from_filters(search_dirs: &[PathBuf]) -> Vec<RewriteRule> {
    let mut rules = Vec::new();
    let mut seen_patterns: std::collections::HashSet<String> = std::collections::HashSet::new();

    let Ok(filters) = config::cache::discover_with_cache(search_dirs) else {
        return rules;
    };

    for filter in filters {
        for pattern in filter.config.command.patterns() {
            if !seen_patterns.insert(pattern.clone()) {
                continue;
            }

            let regex_str = config::command_pattern_to_regex(pattern);
            rules.push(RewriteRule {
                match_pattern: regex_str,
                replace: "tokf run {0}".to_string(),
            });
        }
    }

    rules
}

/// Top-level rewrite function. Orchestrates skip check, user rules, and filter rules.
pub fn rewrite(command: &str) -> String {
    let user_config = load_user_config().unwrap_or_default();
    rewrite_with_config(command, &user_config, &config::default_search_dirs())
}

/// Testable version with explicit config and search dirs.
pub(crate) fn rewrite_with_config(
    command: &str,
    user_config: &RewriteConfig,
    search_dirs: &[PathBuf],
) -> String {
    let user_skip_patterns = user_config
        .skip
        .as_ref()
        .map_or(&[] as &[String], |s| &s.patterns);

    if should_skip(command, user_skip_patterns) {
        return command.to_string();
    }

    let user_result = apply_rules(&user_config.rewrite, command);
    if user_result != command {
        return user_result;
    }

    let filter_rules = build_rules_from_filters(search_dirs);
    let segments = split_compound(command);
    if segments.len() == 1 {
        return apply_rules(&filter_rules, command);
    }

    // Compound command: rewrite each segment independently so every sub-command
    // that has a matching filter is wrapped, not just the first one.
    let mut changed = false;
    let mut out = String::with_capacity(command.len() + segments.len() * 9);
    for (seg, sep) in &segments {
        let trimmed = seg.trim();
        let rewritten = if trimmed.is_empty() || should_skip(trimmed, user_skip_patterns) {
            trimmed.to_string()
        } else {
            let r = apply_rules(&filter_rules, trimmed);
            if r != trimmed {
                changed = true;
            }
            r
        };
        out.push_str(&rewritten);
        out.push_str(sep);
    }
    if changed { out } else { command.to_string() }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    // --- build_rules_from_filters ---

    #[test]
    fn build_rules_from_empty_dir() {
        let dir = TempDir::new().unwrap();
        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        // Empty disk dir — embedded stdlib is always present
        assert!(
            !rules.is_empty(),
            "embedded stdlib should provide built-in rules"
        );
    }

    #[test]
    fn build_rules_from_filter_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();
        fs::write(
            dir.path().join("cargo-test.toml"),
            "command = \"cargo test\"",
        )
        .unwrap();

        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        let patterns: Vec<&str> = rules.iter().map(|r| r.match_pattern.as_str()).collect();

        let has_cargo = patterns
            .iter()
            .any(|p| p.contains("cargo") && p.contains("test"));
        let has_git = patterns
            .iter()
            .any(|p| p.contains("git") && p.contains("status"));
        assert!(has_cargo, "expected cargo test pattern in {:?}", patterns);
        assert!(has_git, "expected git status pattern in {:?}", patterns);

        let cargo_rule = rules
            .iter()
            .find(|r| r.match_pattern.contains("cargo"))
            .unwrap();
        let git_rule = rules
            .iter()
            .find(|r| r.match_pattern.contains("status"))
            .unwrap();
        let re_cargo = regex::Regex::new(&cargo_rule.match_pattern).unwrap();
        let re_git = regex::Regex::new(&git_rule.match_pattern).unwrap();
        assert!(re_cargo.is_match("cargo test"));
        assert!(re_cargo.is_match("cargo test --lib"));
        assert!(re_git.is_match("git status"));
        assert!(re_git.is_match("git status --short"));
    }

    #[test]
    fn build_rules_dedup_across_dirs() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        fs::write(
            dir1.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();
        fs::write(
            dir2.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let rules =
            build_rules_from_filters(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
        let git_status_count = rules
            .iter()
            .filter(|r| r.match_pattern.contains("git") && r.match_pattern.contains("status"))
            .count();
        assert_eq!(
            git_status_count, 1,
            "git status should be deduped to one rule"
        );
    }

    #[test]
    fn build_rules_skips_invalid_filters() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("bad.toml"), "not valid [[[").unwrap();
        fs::write(dir.path().join("good.toml"), "command = \"my-tool\"").unwrap();

        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        assert!(
            rules.iter().any(|r| r.match_pattern.contains("my\\-tool")),
            "expected my-tool rule in {:?}",
            rules.iter().map(|r| &r.match_pattern).collect::<Vec<_>>()
        );
    }

    #[test]
    fn build_rules_from_nested_dirs() {
        let dir = TempDir::new().unwrap();
        let git_dir = dir.path().join("git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("push.toml"), "command = \"git push\"").unwrap();
        fs::write(git_dir.join("status.toml"), "command = \"git status\"").unwrap();

        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        let patterns: Vec<&str> = rules.iter().map(|r| r.match_pattern.as_str()).collect();
        assert!(patterns.iter().any(|p| p.contains("push")));
        assert!(patterns.iter().any(|p| p.contains("status")));
    }

    #[test]
    fn build_rules_multiple_command_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("test-runner.toml"),
            r#"command = ["pnpm test", "npm test"]"#,
        )
        .unwrap();

        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        let patterns: Vec<&str> = rules.iter().map(|r| r.match_pattern.as_str()).collect();
        assert!(patterns.iter().any(|p| p.contains("pnpm")));
        assert!(
            patterns
                .iter()
                .any(|p| p.contains("npm") && !p.contains("pnpm"))
        );
    }

    #[test]
    fn build_rules_wildcard_pattern() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("npm-run.toml"), r#"command = "npm run *""#).unwrap();

        let rules = build_rules_from_filters(&[dir.path().to_path_buf()]);
        let npm_run_rule = rules
            .iter()
            .find(|r| r.match_pattern.contains("npm") && r.match_pattern.contains("run"))
            .expect("expected npm run rule");
        let re = regex::Regex::new(&npm_run_rule.match_pattern).unwrap();
        assert!(re.is_match("npm run build"));
        assert!(re.is_match("npm run test"));
        assert!(!re.is_match("npm install"));
    }

    // --- rewrite_with_config (single command) ---

    #[test]
    fn rewrite_with_filter_match() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig::default();
        let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "tokf run git status");
    }

    #[test]
    fn rewrite_with_filter_match_with_args() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig::default();
        let result =
            rewrite_with_config("git status --short", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "tokf run git status --short");
    }

    #[test]
    fn rewrite_builtin_skip_tokf() {
        let dir = TempDir::new().unwrap();
        let config = RewriteConfig::default();
        let result =
            rewrite_with_config("tokf run git status", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "tokf run git status");
    }

    #[test]
    fn rewrite_no_match_passthrough() {
        let dir = TempDir::new().unwrap();
        let config = RewriteConfig::default();
        let result = rewrite_with_config("unknown-cmd foo", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "unknown-cmd foo");
    }

    #[test]
    fn rewrite_user_rule_takes_priority() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig {
            skip: None,
            rewrite: vec![RewriteRule {
                match_pattern: "^git status".to_string(),
                replace: "custom-wrapper {0}".to_string(),
            }],
        };
        let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "custom-wrapper git status");
    }

    #[test]
    fn rewrite_user_skip_prevents_rewrite() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig {
            skip: Some(types::SkipConfig {
                patterns: vec!["^git status".to_string()],
            }),
            rewrite: vec![],
        };
        let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()]);
        assert_eq!(result, "git status");
    }

    // --- rewrite_with_config (compound commands) ---

    #[test]
    fn rewrite_compound_both_segments_match() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig::default();
        let r = rewrite_with_config(
            "git add foo && git status",
            &config,
            &[dir.path().to_path_buf()],
        );
        assert_eq!(r, "tokf run git add foo && tokf run git status");
    }

    #[test]
    fn rewrite_compound_partial_match() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("git-status.toml"),
            "command = \"git status\"",
        )
        .unwrap();

        let config = RewriteConfig::default();
        let r = rewrite_with_config(
            "unknown-cmd && git status",
            &config,
            &[dir.path().to_path_buf()],
        );
        assert_eq!(r, "unknown-cmd && tokf run git status");
    }

    #[test]
    fn rewrite_compound_pipe_not_split() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

        let config = RewriteConfig::default();
        let r = rewrite_with_config(
            "git diff HEAD | head -5",
            &config,
            &[dir.path().to_path_buf()],
        );
        // Pipe is NOT a chain separator — the whole string is one segment.
        assert_eq!(r, "tokf run git diff HEAD | head -5");
    }

    #[test]
    fn rewrite_compound_no_match_passthrough() {
        let dir = TempDir::new().unwrap();
        let config = RewriteConfig::default();
        let r = rewrite_with_config(
            "unknown-a && unknown-b",
            &config,
            &[dir.path().to_path_buf()],
        );
        assert_eq!(r, "unknown-a && unknown-b");
    }
}
