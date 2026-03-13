#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use tempfile::TempDir;

use super::*;

// --- collect_filter_patterns ---

#[test]
fn collect_patterns_from_empty_dir() {
    let dir = TempDir::new().unwrap();
    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);
    // Empty disk dir — embedded stdlib is always present
    assert!(
        !patterns.is_empty(),
        "embedded stdlib should provide built-in patterns"
    );
}

#[test]
fn collect_patterns_from_filter_files() {
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

    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);

    let has_cargo = patterns
        .iter()
        .any(|p| p.contains("cargo") && p.contains("test"));
    let has_git = patterns
        .iter()
        .any(|p| p.contains("git") && p.contains("status"));
    assert!(has_cargo, "expected cargo test pattern in {patterns:?}");
    assert!(has_git, "expected git status pattern in {patterns:?}");
}

#[test]
fn collect_patterns_dedup_across_dirs() {
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

    let patterns = collect_filter_patterns(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
    let git_status_count = patterns.iter().filter(|p| *p == "git status").count();
    assert_eq!(
        git_status_count, 1,
        "git status should be deduped to one pattern"
    );
}

#[test]
fn collect_patterns_skips_invalid_filters() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.toml"), "not valid [[[").unwrap();
    fs::write(dir.path().join("good.toml"), "command = \"my-tool\"").unwrap();

    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);
    assert!(
        patterns.iter().any(|p| p == "my-tool"),
        "expected my-tool pattern in {patterns:?}",
    );
}

#[test]
fn collect_patterns_from_nested_dirs() {
    let dir = TempDir::new().unwrap();
    let git_dir = dir.path().join("git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("push.toml"), "command = \"git push\"").unwrap();
    fs::write(git_dir.join("status.toml"), "command = \"git status\"").unwrap();

    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);
    assert!(patterns.iter().any(|p| p.contains("push")));
    assert!(patterns.iter().any(|p| p.contains("status")));
}

#[test]
fn collect_patterns_multiple_command_patterns() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);
    assert!(patterns.iter().any(|p| p.contains("pnpm")));
    assert!(
        patterns
            .iter()
            .any(|p| p.contains("npm") && !p.contains("pnpm"))
    );
}

#[test]
fn collect_patterns_wildcard_pattern() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("npm-run.toml"), r#"command = "npm run *""#).unwrap();

    let patterns = collect_filter_patterns(&[dir.path().to_path_buf()]);
    assert!(
        patterns.iter().any(|p| p == "npm run *"),
        "expected raw wildcard pattern in {patterns:?}",
    );
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

// --- interleaved flags (the bug fix) ---

#[test]
fn rewrite_interleaved_flags_pnpm_build() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("pnpm-build.toml"),
        "command = \"pnpm build\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    // pnpm --dir apps/web build should match pattern "pnpm build"
    let result = rewrite_with_config(
        "pnpm --dir apps/web build",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run pnpm --dir apps/web build");
}

#[test]
fn rewrite_interleaved_flags_git_c_status() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    // git -C /some/path status should match pattern "git status"
    let result = rewrite_with_config(
        "git -C /some/path status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(result, "tokf run git -C /some/path status");
}

// --- built-in wrapper rules (make, just) ---

#[test]
fn wrapper_make_with_args() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("make check", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "make SHELL=tokf check");
}

#[test]
fn wrapper_make_no_args() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("make", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "make SHELL=tokf");
}

#[test]
fn wrapper_make_full_path() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "/usr/bin/make check",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "make SHELL=tokf check");
}

#[test]
fn wrapper_just_with_args() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("just test", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "just --shell tokf --shell-arg -cu test");
}

#[test]
fn wrapper_just_no_args() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("just", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "just --shell tokf --shell-arg -cu");
}

#[test]
fn wrapper_just_full_path() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "/usr/local/bin/just test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "just --shell tokf --shell-arg -cu test");
}

#[test]
fn wrapper_user_rule_overrides_builtin_wrapper() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: None,
        pipe: None,
        rewrite: vec![RewriteRule {
            match_pattern: r"^make(\s.*)?$".to_string(),
            replace: "custom-make{1}".to_string(),
        }],
    };
    let r = rewrite_with_config("make check", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "custom-make check");
}

#[test]
fn wrapper_skip_pattern_prevents_wrapper() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig {
        skip: Some(types::SkipConfig {
            patterns: vec!["^make".to_string()],
        }),
        pipe: None,
        rewrite: vec![],
    };
    let r = rewrite_with_config("make check", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "make check");
}

#[test]
fn wrapper_make_in_compound() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("git-status.toml"),
        "command = \"git status\"",
    )
    .unwrap();

    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "make check && git status",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "make SHELL=tokf check && tokf run git status");
}

#[test]
fn wrapper_env_prefix_preserved() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "MAKEFLAGS=-j4 make check",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "MAKEFLAGS=-j4 make SHELL=tokf check");
}

// --- negative: commands that must NOT match wrapper rules ---

#[test]
fn wrapper_cmake_not_rewritten() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "cmake --build .",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "cmake --build .");
}

#[test]
fn wrapper_remake_not_rewritten() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("remake check", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "remake check");
}

#[test]
fn wrapper_justfile_not_rewritten() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("justfile test", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "justfile test");
}

#[test]
fn wrapper_adjust_not_rewritten() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config("adjust params", &config, &[dir.path().to_path_buf()], false);
    assert_eq!(r, "adjust params");
}

// --- wrapper edge cases ---

#[test]
fn wrapper_make_with_pipe_preserves_pipe() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "make check | tee log.txt",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "make SHELL=tokf check | tee log.txt");
}

#[test]
fn wrapper_two_wrappers_in_compound() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "just test && make check",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(
        r,
        "just --shell tokf --shell-arg -cu test && make SHELL=tokf check"
    );
}

#[test]
fn wrapper_env_prefix_with_just() {
    let dir = TempDir::new().unwrap();
    let config = RewriteConfig::default();
    let r = rewrite_with_config(
        "CI=true just test",
        &config,
        &[dir.path().to_path_buf()],
        false,
    );
    assert_eq!(r, "CI=true just --shell tokf --shell-arg -cu test");
}

#[test]
fn wrapper_build_rules_count() {
    let rules = build_wrapper_rules();
    assert_eq!(rules.len(), 2, "expected 2 built-in wrappers (make, just)");
    // Verify regexes compile.
    for rule in &rules {
        regex::Regex::new(&rule.match_pattern).expect("built-in wrapper regex should compile");
    }
}
