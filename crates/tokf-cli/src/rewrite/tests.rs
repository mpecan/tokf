#![allow(clippy::unwrap_used, clippy::expect_used)]

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
    assert!(has_cargo, "expected cargo test pattern in {patterns:?}");
    assert!(has_git, "expected git status pattern in {patterns:?}");

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

    let rules = build_rules_from_filters(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
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
    let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()], false);
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
    let result = rewrite_with_config(
        "git status --short",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run git status --short");
}

#[test]
fn rewrite_builtin_skip_tokf() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "tokf run git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run git status");
}

#[test]
fn rewrite_no_match_passthrough() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "unknown-cmd foo",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
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
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^git status".to_string(),
            replace: "custom-wrapper {0}".to_string(),
        }],
    };
    let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()], false);
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
        pipe: None,
        rewrite: vec![],
    };
    let result = rewrite_with_config("git status", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(result, "git status");
}

#[test]
fn rewrite_transparent_global_flag() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-log.toml"), "command = \"git log\"").unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "git -C /path log --oneline",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run git -C /path log --oneline");
}

#[test]
fn rewrite_basename_full_path() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "/usr/bin/git status --short",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run /usr/bin/git status --short");
}

#[test]
fn rewrite_basename_and_transparent_flags_combined() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-log.toml"), "command = \"git log\"").unwrap();

    let config = RewriteConfig::default();
    let result = rewrite_with_config(
        "/usr/bin/git --no-pager -C /repo log --oneline",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        result,
        "tokf run /usr/bin/git --no-pager -C /repo log --oneline"
    );
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
        false,
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
        false,
    );
    assert_eq!(r, "unknown-cmd && tokf run git status");
}

#[test]
fn rewrite_pipe_head_stripped_when_filter_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git diff HEAD | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // Simple pipe to head stripped — tokf filter provides structured output.
    assert_eq!(r, "tokf run --baseline-pipe 'head -5' git diff HEAD");
}

#[test]
fn rewrite_pipe_grep_stripped_when_filter_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | grep FAILED",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "tokf run --baseline-pipe 'grep FAILED' cargo test");
}

#[test]
fn rewrite_pipe_no_filter_preserves_pipe() {
    let dir = TempDir::new().unwrap();
    // No filter for "unknown-cmd"
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "unknown-cmd | tail -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "unknown-cmd | tail -5");
}

#[test]
fn rewrite_pipe_wc_passes_through() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git status | wc -l",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // wc is not a strippable target — pipe passes through.
    assert_eq!(r, "git status | wc -l");
}

#[test]
fn rewrite_pipe_tail_follow_passes_through() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | tail -f",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // tail -f (follow mode) is not strippable.
    assert_eq!(r, "cargo test | tail -f");
}

#[test]
fn rewrite_logical_or_still_rewritten() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test || echo failed",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "tokf run cargo test || echo failed");
}

#[test]
fn rewrite_multi_pipe_chain_not_rewritten() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git status | grep M | wc -l",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "git status | grep M | wc -l");
}

#[test]
fn rewrite_compound_no_match_passthrough() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "unknown-a && unknown-b",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "unknown-a && unknown-b");
}

#[test]
fn rewrite_user_rule_wraps_piped_command() {
    // User-configured rules run before the pipe guard, so they CAN wrap piped commands.
    // Using {0}{rest} captures both the matched portion and the remainder (including the pipe).
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: "^cargo test".to_string(),
            replace: "my-wrapper {0}{rest}".to_string(),
        }],
    };
    let r = rewrite_with_config(
        "cargo test | grep FAILED",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "my-wrapper cargo test| grep FAILED");
}

#[test]
fn rewrite_skip_pattern_wins_over_pipe_guard() {
    // should_skip is checked first; a piped command that matches a skip pattern
    // returns early before the pipe guard even runs.
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: Some(types::SkipConfig {
            patterns: vec!["^git".to_string()],
        }),
        pipe: None,
        rewrite: vec![],
    };
    let r = rewrite_with_config(
        "git status | grep M",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "git status | grep M");
}

#[test]
fn rewrite_compound_then_pipe_stripped() {
    // A compound command where one segment has a strippable pipe: each segment
    // is rewritten independently, and the pipe in the second segment is stripped.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("git-add.toml"), "command = \"git add\"").unwrap();
    fs::write(dir.path().join("git-diff.toml"), "command = \"git diff\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "git add . && git diff | head -5",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run git add . && tokf run --baseline-pipe 'head -5' git diff"
    );
}

#[test]
fn rewrite_quoted_pipe_is_not_a_bare_pipe() {
    // A pipe inside quotes is not a shell pipe operator — the command should be rewritten.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("grep.toml"), "command = \"grep\"").unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "grep -E 'foo|bar' file.txt",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    // No bare pipe — the filter rule should fire.
    assert_eq!(r, "tokf run grep -E 'foo|bar' file.txt");
}

#[test]
fn rewrite_pipe_grep_quoted_pattern_escaped() {
    // Single quotes in the grep suffix must be escaped with '\'' in the
    // --baseline-pipe value so the shell command remains valid.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("cargo-test.toml"),
        "command = \"cargo test\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cargo test | grep -E 'fail|error'",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "tokf run --baseline-pipe 'grep -E '\\''fail|error'\\''' cargo test"
    );
}
