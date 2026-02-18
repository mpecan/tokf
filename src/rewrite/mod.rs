pub mod types;

use std::path::PathBuf;

use regex::Regex;

use crate::config;
use types::{RewriteConfig, RewriteRule};

/// Built-in skip patterns that are always active.
/// - `^tokf ` prevents double-wrapping
/// - `<<` prevents rewriting heredocs
const BUILTIN_SKIP_PATTERNS: &[&str] = &["^tokf ", "<<"];

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

    let Ok(filters) = config::discover_all_filters(search_dirs) else {
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

/// Search config dirs for `rewrites.toml` (first found wins).
///
/// Search order:
/// 1. `.tokf/rewrites.toml` (project-local)
/// 2. `~/.config/tokf/rewrites.toml` (user-level)
pub fn load_user_config() -> Option<RewriteConfig> {
    load_user_config_from(&config_search_paths())
}

/// Config search paths for `rewrites.toml`.
fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".tokf/rewrites.toml"));
    }

    if let Some(config) = dirs::config_dir() {
        paths.push(config.join("tokf/rewrites.toml"));
    }

    paths
}

/// Testable version that accepts explicit paths.
fn load_user_config_from(paths: &[PathBuf]) -> Option<RewriteConfig> {
    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            match toml::from_str(&content) {
                Ok(config) => return Some(config),
                Err(e) => {
                    eprintln!("[tokf] warning: failed to parse {}: {e}", path.display());
                    return None;
                }
            }
        }
    }
    None
}

/// Check if a command should be skipped (not rewritten).
pub(crate) fn should_skip(command: &str, user_patterns: &[String]) -> bool {
    for pattern in BUILTIN_SKIP_PATTERNS {
        if let Ok(re) = Regex::new(pattern)
            && re.is_match(command)
        {
            return true;
        }
    }

    for pattern in user_patterns {
        match Regex::new(pattern) {
            Ok(re) if re.is_match(command) => return true,
            Err(e) => {
                eprintln!("[tokf] warning: invalid skip pattern \"{pattern}\": {e}");
            }
            _ => {}
        }
    }

    false
}

/// Apply the first matching rewrite rule. Returns the original command if none match.
pub(crate) fn apply_rules(rules: &[RewriteRule], command: &str) -> String {
    for rule in rules {
        let Ok(re) = Regex::new(&rule.match_pattern) else {
            continue;
        };

        if let Some(caps) = re.captures(command) {
            return interpolate_rewrite(&rule.replace, &caps, command);
        }
    }

    command.to_string()
}

/// Interpolate `{0}`, `{1}`, `{2}`, ... and `{rest}` in the replacement template.
fn interpolate_rewrite(template: &str, caps: &regex::Captures<'_>, full_input: &str) -> String {
    let mut result = template.to_string();

    // Handle the {rest} placeholder — text after the entire match
    let rest = &full_input[caps.get(0).map_or(full_input.len(), |m| m.end())..];
    let rest = rest.trim_start();
    #[allow(clippy::literal_string_with_formatting_args)]
    let rest_token = "{rest}";
    result = result.replace(rest_token, rest);

    // Handle numbered groups in reverse order (so {10} is replaced before {1})
    let max_group = caps.len().saturating_sub(1);
    for i in (0..=max_group).rev() {
        let placeholder = format!("{{{i}}}");
        let value = caps.get(i).map_or("", |m| m.as_str());
        result = result.replace(&placeholder, value);
    }

    result
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
    apply_rules(&filter_rules, command)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    // --- should_skip ---

    #[test]
    fn skip_tokf_commands() {
        assert!(should_skip("tokf run git status", &[]));
        assert!(should_skip("tokf rewrite foo", &[]));
    }

    #[test]
    fn skip_heredocs() {
        assert!(should_skip("cat <<EOF", &[]));
        assert!(should_skip("bash -c 'cat <<EOF'", &[]));
    }

    #[test]
    fn skip_user_patterns() {
        let patterns = vec!["^my-internal".to_string()];
        assert!(should_skip("my-internal tool", &patterns));
        assert!(!should_skip("git status", &patterns));
    }

    #[test]
    fn skip_invalid_user_pattern_does_not_crash() {
        // Invalid regex should produce a warning but not skip or crash
        let patterns = vec!["[invalid".to_string()];
        assert!(!should_skip("git status", &patterns));
    }

    #[test]
    fn no_skip_normal_commands() {
        assert!(!should_skip("git status", &[]));
        assert!(!should_skip("cargo test", &[]));
        assert!(!should_skip("ls -la", &[]));
    }

    // --- apply_rules ---

    #[test]
    fn apply_rules_first_match_wins() {
        let rules = vec![
            RewriteRule {
                match_pattern: "^git status".to_string(),
                replace: "first {0}".to_string(),
            },
            RewriteRule {
                match_pattern: "^git".to_string(),
                replace: "second {0}".to_string(),
            },
        ];
        assert_eq!(apply_rules(&rules, "git status"), "first git status");
    }

    #[test]
    fn apply_rules_no_match_returns_original() {
        let rules = vec![RewriteRule {
            match_pattern: "^git".to_string(),
            replace: "tokf run {0}".to_string(),
        }];
        assert_eq!(apply_rules(&rules, "ls -la"), "ls -la");
    }

    #[test]
    fn apply_rules_empty_rules_returns_original() {
        assert_eq!(apply_rules(&[], "git status"), "git status");
    }

    #[test]
    fn apply_rules_capture_groups() {
        let rules = vec![RewriteRule {
            match_pattern: r"^(git) (status)".to_string(),
            replace: "wrapped {1} {2}".to_string(),
        }];
        assert_eq!(apply_rules(&rules, "git status"), "wrapped git status");
    }

    #[test]
    fn apply_rules_invalid_regex_skipped() {
        let rules = vec![
            RewriteRule {
                match_pattern: "[invalid".to_string(),
                replace: "bad".to_string(),
            },
            RewriteRule {
                match_pattern: r"^git status(\s.*)?$".to_string(),
                replace: "tokf run {0}".to_string(),
            },
        ];
        assert_eq!(apply_rules(&rules, "git status"), "tokf run git status");
    }

    // --- interpolate_rewrite ---

    #[test]
    fn interpolate_full_match() {
        let re = Regex::new(r"^git status(\s.*)?$").unwrap();
        let caps = re.captures("git status --short").unwrap();
        let result = interpolate_rewrite("tokf run {0}", &caps, "git status --short");
        assert_eq!(result, "tokf run git status --short");
    }

    #[test]
    fn interpolate_rest() {
        let re = Regex::new(r"^git status").unwrap();
        let caps = re.captures("git status --short -b").unwrap();
        let result =
            interpolate_rewrite("tokf run git status {rest}", &caps, "git status --short -b");
        assert_eq!(result, "tokf run git status --short -b");
    }

    #[test]
    fn interpolate_rest_empty() {
        let re = Regex::new(r"^git status$").unwrap();
        let caps = re.captures("git status").unwrap();
        let result = interpolate_rewrite("tokf run git status {rest}", &caps, "git status");
        assert_eq!(result, "tokf run git status ");
    }

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
        // Disk-based rules plus embedded stdlib may be present
        let patterns: Vec<&str> = rules.iter().map(|r| r.match_pattern.as_str()).collect();

        // Both cargo test and git status patterns should be present
        let has_cargo = patterns
            .iter()
            .any(|p| p.contains("cargo") && p.contains("test"));
        let has_git = patterns
            .iter()
            .any(|p| p.contains("git") && p.contains("status"));
        assert!(has_cargo, "expected cargo test pattern in {:?}", patterns);
        assert!(has_git, "expected git status pattern in {:?}", patterns);

        // Both should match with trailing args
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
        // git status should appear exactly once even though it's in both dirs
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
        // bad.toml is skipped; my-tool should be present
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
        // Two command patterns → two disk-based rules (plus any embedded stdlib)
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

    // --- rewrite_with_config ---

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

    // --- load_user_config_from ---

    #[test]
    fn load_config_first_found_wins() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        let path1 = dir1.path().join("rewrites.toml");
        let path2 = dir2.path().join("rewrites.toml");

        fs::write(
            &path1,
            r#"
[[rewrite]]
match = "^first"
replace = "first"
"#,
        )
        .unwrap();
        fs::write(
            &path2,
            r#"
[[rewrite]]
match = "^second"
replace = "second"
"#,
        )
        .unwrap();

        let config = load_user_config_from(&[path1, path2]).unwrap();
        assert_eq!(config.rewrite[0].match_pattern, "^first");
    }

    #[test]
    fn load_config_nonexistent_returns_none() {
        let result = load_user_config_from(&[PathBuf::from("/no/such/file.toml")]);
        assert!(result.is_none());
    }

    #[test]
    fn load_config_invalid_toml_returns_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rewrites.toml");
        fs::write(&path, "not valid [[[").unwrap();

        let result = load_user_config_from(&[path]);
        assert!(result.is_none());
    }
}
