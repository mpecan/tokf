#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

// --- pattern_specificity ---

#[test]
fn specificity_two_literals() {
    assert_eq!(pattern_specificity("git push"), 2);
}

#[test]
fn specificity_wildcard_counts_less() {
    assert_eq!(pattern_specificity("git *"), 1);
    assert_eq!(pattern_specificity("* push"), 1);
}

#[test]
fn specificity_all_wildcards() {
    assert_eq!(pattern_specificity("* *"), 0);
}

#[test]
fn specificity_ordering() {
    // "git push" more specific than "git *" more specific than "* push"
    assert!(pattern_specificity("git push") > pattern_specificity("git *"));
    assert!(pattern_specificity("git *") == pattern_specificity("* push"));
}

// --- pattern_matches_prefix ---

#[test]
fn matches_exact() {
    let words = ["git", "push"];
    assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
}

#[test]
fn matches_prefix_with_trailing_args() {
    let words = ["git", "push", "origin", "main"];
    assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
}

#[test]
fn matches_wildcard() {
    let words = ["npm", "run", "build"];
    assert_eq!(pattern_matches_prefix("npm run *", &words), Some(3));
}

#[test]
fn no_match_different_command() {
    let words = ["cargo", "test"];
    assert_eq!(pattern_matches_prefix("git push", &words), None);
}

#[test]
fn no_match_too_short() {
    let words = ["git"];
    assert_eq!(pattern_matches_prefix("git push", &words), None);
}

#[test]
fn empty_pattern_returns_none() {
    let words = ["git", "push"];
    assert_eq!(pattern_matches_prefix("", &words), None);
}

#[test]
fn empty_words_returns_none() {
    assert_eq!(pattern_matches_prefix("git push", &[]), None);
}

#[test]
fn single_word_pattern_prefix_match() {
    assert_eq!(pattern_matches_prefix("echo", &["echo"]), Some(1));
    assert_eq!(pattern_matches_prefix("echo", &["echo", "hello"]), Some(1));
    assert_eq!(pattern_matches_prefix("echo", &["ls"]), None);
}

#[test]
fn wildcard_rejects_empty_token() {
    // An empty string slice element is not a valid word match for `*`
    assert_eq!(pattern_matches_prefix("git *", &["git", ""]), None);
}

#[test]
fn wildcard_at_start() {
    let words = ["my-tool", "subcommand"];
    assert_eq!(pattern_matches_prefix("* subcommand", &words), Some(2));
}

#[test]
fn hyphenated_tool_not_ambiguous() {
    // golangci-lint run should match "golangci-lint run" but not "golangci-lint"
    let words = ["golangci-lint", "run"];
    assert_eq!(pattern_matches_prefix("golangci-lint run", &words), Some(2));
    assert_eq!(pattern_matches_prefix("golangci-lint", &words), Some(1));
}

#[test]
fn basename_matching() {
    assert_eq!(pattern_matches_prefix("ls", &["/usr/bin/ls"]), Some(1));
    assert_eq!(
        pattern_matches_prefix("ls -la", &["/usr/bin/ls", "-la"]),
        Some(2)
    );
    assert_eq!(pattern_matches_prefix("mvnw", &["./mvnw"]), Some(1));
    assert_eq!(
        pattern_matches_prefix("git push", &["git", "push"]),
        Some(2)
    );
    assert_eq!(pattern_matches_prefix("git /p", &["git", "/p"]), Some(2));
    assert_eq!(pattern_matches_prefix("git f", &["git", "/p"]), None);
}

// --- extract_basename ---

#[test]
fn basename_plain_word() {
    assert_eq!(extract_basename("git"), "git");
}

#[test]
fn basename_absolute_path() {
    assert_eq!(extract_basename("/usr/bin/git"), "git");
}

#[test]
fn basename_relative_path() {
    assert_eq!(extract_basename("./mvnw"), "mvnw");
}

#[test]
fn basename_windows_path() {
    assert_eq!(extract_basename(r"C:\project\mvnw"), "mvnw");
}

#[test]
fn basename_trailing_separator_returns_empty() {
    // e.g. a path accidentally ending in /
    assert_eq!(extract_basename("git/"), "");
}

#[test]
fn basename_root_returns_empty() {
    assert_eq!(extract_basename("/"), "");
}

#[test]
fn basename_empty_string() {
    assert_eq!(extract_basename(""), "");
}

// --- skip_flags_to_match ---

#[test]
fn skip_flags_short_flag_with_value() {
    // -C /path: flag + separate value, then target
    let words = ["-C", "/path", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_long_flag_no_value() {
    // --no-pager is a standalone flag (next word is target)
    let words = ["--no-pager", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_long_flag_equals_value() {
    // --format=%s: entire token is flag+value
    let words = ["--format=%s", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_multiple_flags() {
    let words = ["--no-pager", "-C", "/path", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(4));
}

#[test]
fn skip_flags_target_first() {
    // target is the very first word — consumed immediately
    let words = ["log", "--oneline"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(1));
}

#[test]
fn skip_flags_non_flag_non_target_blocks() {
    // "something" is not a flag and not the target → None
    let words = ["something", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), None);
}

#[test]
fn skip_flags_empty_words() {
    assert_eq!(skip_flags_to_match(&[], "log"), None);
}

#[test]
fn skip_flags_flag_value_is_target() {
    // -C log: "/path" slot is occupied by "log" itself — it IS the target,
    // so we should NOT skip it as a value.
    // Words: ["-C", "log", "log"] — first "log" should be detected as target.
    let words = ["-C", "log", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_combined_form_no_space() {
    // -Cpath is treated as a single flag token (no separate value), then target
    let words = ["-Cpath", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(2));
}

#[test]
fn skip_flags_flag_value_looks_like_flag() {
    // -C -v: since -v starts with '-', it is NOT consumed as -C's value —
    // it is treated as the next standalone flag instead.
    let words = ["-C", "-v", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_multiple_equals_style_flags() {
    // Two --flag=value tokens in a row, then target
    let words = ["--fmt=%s", "--depth=5", "log"];
    assert_eq!(skip_flags_to_match(&words, "log"), Some(3));
}

#[test]
fn skip_flags_flag_only_at_end_no_target() {
    // Flag at end of slice, target never found
    let words = ["-C"];
    assert_eq!(skip_flags_to_match(&words, "log"), None);
}

// --- transparent global args in pattern_matches_prefix ---

#[test]
fn transparent_short_flag_with_value() {
    // git -C /path log  →  matches "git log", consumed = 4
    let words = ["git", "-C", "/path", "log", "--oneline"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(4));
}

#[test]
fn transparent_long_flag_no_value() {
    // git --no-pager log  →  consumed = 3
    let words = ["git", "--no-pager", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(3));
}

#[test]
fn transparent_flag_equals_value() {
    // git --format=%s log  →  consumed = 3
    let words = ["git", "--format=%s", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(3));
}

#[test]
fn transparent_multiple_flags() {
    // git --no-pager -C /path log  →  consumed = 5
    let words = ["git", "--no-pager", "-C", "/path", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(5));
}

#[test]
fn transparent_no_skip_for_non_flags() {
    // "somedir" is not a flag — should not match
    let words = ["git", "somedir", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), None);
}

#[test]
fn transparent_direct_match_unchanged() {
    // No transparent args — existing behaviour preserved
    let words = ["git", "log", "--oneline"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(2));
}

#[test]
fn transparent_combined_with_basename() {
    // /usr/bin/git -C /path log  →  basename match + transparent skip
    let words = ["/usr/bin/git", "-C", "/path", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), Some(4));
}

#[test]
fn transparent_wildcard_with_flags_before_literal() {
    // npm --prefix /path run build  →  pattern "npm run *", consumed = 5
    let words = ["npm", "--prefix", "/path", "run", "build"];
    assert_eq!(pattern_matches_prefix("npm run *", &words), Some(5));
}

#[test]
fn transparent_flags_not_skipped_at_first_word() {
    // Transparent skipping only happens after the first word
    // A command starting with a flag should not match
    let words = ["--no-pager", "git", "log"];
    assert_eq!(pattern_matches_prefix("git log", &words), None);
}

// --- discover_filter_files ---

#[test]
fn discover_flat_dir() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.toml"), "").unwrap();
    fs::write(dir.path().join("b.toml"), "").unwrap();
    fs::write(dir.path().join("not-toml.txt"), "").unwrap();

    let files = discover_filter_files(dir.path());
    assert_eq!(files.len(), 2);
    assert!(files[0].ends_with("a.toml"));
    assert!(files[1].ends_with("b.toml"));
}

#[test]
fn discover_nested_dirs() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("git");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("push.toml"), "").unwrap();
    fs::write(sub.join("status.toml"), "").unwrap();
    fs::write(dir.path().join("root.toml"), "").unwrap();

    let files = discover_filter_files(dir.path());
    assert_eq!(files.len(), 3);
    // sorted by path: git/push.toml, git/status.toml, root.toml
    assert!(files[0].ends_with("git/push.toml"));
    assert!(files[1].ends_with("git/status.toml"));
    assert!(files[2].ends_with("root.toml"));
}

#[test]
fn discover_skips_hidden_entries() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".hidden.toml"), "").unwrap();
    fs::write(dir.path().join("visible.toml"), "").unwrap();
    let hidden_dir = dir.path().join(".hiddendir");
    fs::create_dir_all(&hidden_dir).unwrap();
    fs::write(hidden_dir.join("inside.toml"), "").unwrap();

    let files = discover_filter_files(dir.path());
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("visible.toml"));
}

#[test]
fn discover_nonexistent_dir_returns_empty() {
    let files = discover_filter_files(Path::new("/no/such/directory/ever"));
    assert!(files.is_empty());
}

// --- discover_all_filters ---

#[test]
fn discover_all_priority_ordering() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();

    // dir1 = priority 0 (local), dir2 = priority 1 (user)
    fs::write(
        dir1.path().join("my-cmd.toml"),
        "command = \"my cmd local\"",
    )
    .unwrap();
    fs::write(dir2.path().join("my-cmd.toml"), "command = \"my cmd user\"").unwrap();

    let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
    let filters = discover_all_filters(&dirs).unwrap();

    // Should have both (different command strings) plus embedded stdlib
    assert!(filters.len() >= 2);
    assert_eq!(filters[0].config.command.first(), "my cmd local");
    assert_eq!(filters[0].priority, 0);
}

#[test]
fn discover_all_dedup_same_command() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();

    fs::write(dir1.path().join("a.toml"), "command = \"git push\"").unwrap();
    fs::write(dir2.path().join("b.toml"), "command = \"git push\"").unwrap();

    let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
    let filters = discover_all_filters(&dirs).unwrap();

    // Dedup by first() — only one entry for "git push"
    let push_entries: Vec<_> = filters
        .iter()
        .filter(|f| f.config.command.first() == "git push")
        .collect();
    assert_eq!(push_entries.len(), 1);
    assert_eq!(push_entries[0].priority, 0);
}

#[test]
fn discover_all_specificity_ordering() {
    let dir = TempDir::new().unwrap();

    // More specific patterns should sort first within same priority
    fs::write(dir.path().join("a.toml"), "command = \"git *\"").unwrap();
    fs::write(dir.path().join("b.toml"), "command = \"git push\"").unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = discover_all_filters(&dirs).unwrap();

    // "git push" (specificity=2) should come before "git *" (specificity=1)
    assert_eq!(filters[0].config.command.first(), "git push");
    assert_eq!(filters[1].config.command.first(), "git *");
}

#[test]
fn discover_all_skips_invalid_toml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.toml"), "not valid [[[").unwrap();
    fs::write(dir.path().join("good.toml"), "command = \"my tool\"").unwrap();

    let filters = discover_all_filters(&[dir.path().to_path_buf()]).unwrap();
    let my_tool: Vec<_> = filters
        .iter()
        .filter(|f| f.config.command.first() == "my tool")
        .collect();
    assert_eq!(my_tool.len(), 1);
}

#[test]
fn discover_all_hyphenated_tool_not_ambiguous() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("golangci-lint.toml"),
        "command = \"golangci-lint run\"",
    )
    .unwrap();

    let filters = discover_all_filters(&[dir.path().to_path_buf()]).unwrap();
    let golangci: Vec<_> = filters
        .iter()
        .filter(|f| f.config.command.first() == "golangci-lint run")
        .collect();
    assert_eq!(golangci.len(), 1);
    let words = ["golangci-lint", "run"];
    assert_eq!(golangci[0].matches(&words), Some(2));

    let words_no_match = ["golangci", "lint", "run"];
    assert_eq!(golangci[0].matches(&words_no_match), None);
}

// --- embedded stdlib tests ---

#[test]
fn embedded_stdlib_non_empty() {
    let entries: Vec<_> = STDLIB.find("**/*.toml").unwrap().collect();
    assert!(
        entries.len() >= 10,
        "expected at least 10 embedded filters, got {}",
        entries.len()
    );
}

#[test]
fn all_embedded_toml_parse() {
    for entry in STDLIB.find("**/*.toml").unwrap() {
        if let DirEntry::File(file) = entry {
            let path = file.path();
            // Skip test case files: a file living inside a suite directory, identified by
            // the presence of a sibling <parent_dir_name>.toml next to its parent directory.
            // This mirrors the logic in verify_cmd::collect_suites().
            if let (Some(parent), Some(grandparent)) =
                (path.parent(), path.parent().and_then(Path::parent))
            {
                if let Some(dir_name) = parent.file_name() {
                    let sibling = grandparent.join(format!("{}.toml", dir_name.to_string_lossy()));
                    if STDLIB.get_file(sibling).is_some() {
                        continue;
                    }
                }
            }
            let content = file.contents_utf8().unwrap_or("");
            assert!(
                toml::from_str::<FilterConfig>(content).is_ok(),
                "failed to parse embedded filter: {}",
                path.display()
            );
        }
    }
}

#[test]
fn embedded_filters_in_discover_with_no_dirs() {
    // With empty search dirs, only embedded stdlib is returned
    let filters = discover_all_filters(&[]).unwrap();
    assert!(
        !filters.is_empty(),
        "expected embedded stdlib filters with no search dirs"
    );
    let has_git_push = filters
        .iter()
        .any(|f| f.config.command.first() == "git push");
    assert!(has_git_push, "expected git push in embedded stdlib");
}

#[test]
fn local_filter_shadows_embedded() {
    let dir = TempDir::new().unwrap();
    // Override git push locally
    fs::write(
        dir.path().join("push.toml"),
        "command = \"git push\"\n# local override",
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = discover_all_filters(&dirs).unwrap();

    // "git push" should appear exactly once (local shadows embedded)
    let push_entries: Vec<_> = filters
        .iter()
        .filter(|f| f.config.command.first() == "git push")
        .collect();
    assert_eq!(push_entries.len(), 1);
    assert_eq!(push_entries[0].priority, 0); // local priority
}

// --- try_load_filter ---

#[test]
fn test_load_valid_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.toml");
    fs::write(&path, "command = \"echo hello\"").unwrap();

    let config = try_load_filter(&path).unwrap().unwrap();
    assert_eq!(config.command.first(), "echo hello");
}

#[test]
fn test_load_invalid_toml() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.toml");
    fs::write(&path, "not valid toml [[[").unwrap();

    assert!(try_load_filter(&path).is_err());
}

#[test]
fn test_load_nonexistent_returns_none() {
    let path = PathBuf::from("/tmp/nonexistent-tokf-test-file.toml");
    assert!(try_load_filter(&path).unwrap().is_none());
}

#[test]
fn test_load_real_stdlib_filter() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters/git/push.toml");
    let config = try_load_filter(&path).unwrap().unwrap();
    assert_eq!(config.command.first(), "git push");
}

// --- default_search_dirs ---

#[test]
fn test_default_search_dirs_non_empty_and_starts_with_local() {
    let dirs = default_search_dirs();
    assert!(!dirs.is_empty());
    assert!(
        dirs[0].is_absolute(),
        "first dir should be absolute, got: {:?}",
        dirs[0]
    );
    assert!(
        dirs[0].ends_with(".tokf/filters"),
        "first dir should end with .tokf/filters, got: {:?}",
        dirs[0]
    );
}

#[test]
fn test_default_search_dirs_only_local_and_user() {
    let dirs = default_search_dirs();
    // Should have at most 2 dirs: local (.tokf/filters) and user config
    // The binary-adjacent path has been removed; embedded stdlib replaces it.
    assert!(
        dirs.len() <= 2,
        "expected at most 2 search dirs (local + user), got {}: {:?}",
        dirs.len(),
        dirs
    );
}

// --- command_pattern_to_regex ---

#[test]
fn regex_from_literal_pattern() {
    let r = command_pattern_to_regex("git push");
    let re = regex::Regex::new(&r).unwrap();
    // Basic matches (existing behaviour preserved)
    assert!(re.is_match("git push"));
    assert!(re.is_match("git push origin main"));
    assert!(!re.is_match("git status"));
}

#[test]
fn regex_from_wildcard_pattern() {
    let r = command_pattern_to_regex("npm run *");
    let re = regex::Regex::new(&r).unwrap();
    // Basic matches (existing behaviour preserved)
    assert!(re.is_match("npm run build"));
    assert!(re.is_match("npm run test --watch"));
    assert!(!re.is_match("npm run"));
    assert!(!re.is_match("npm install"));
}

#[test]
fn regex_basename_matching() {
    let r = command_pattern_to_regex("git push");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("/usr/bin/git push"), "full path should match");
    assert!(re.is_match("./git push"), "relative path should match");
    assert!(!re.is_match("git-lfs push"), "git-lfs ≠ git");
}

#[test]
fn regex_transparent_short_flag_with_value() {
    // git -C /path log  →  matches "git log"
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git -C /path log"));
    assert!(re.is_match("git -C /path log --oneline"));
}

#[test]
fn regex_transparent_long_flag_no_value() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --no-pager log"));
    assert!(re.is_match("git --no-pager log --oneline"));
}

#[test]
fn regex_transparent_flag_equals_value() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --format=%s log"));
}

#[test]
fn regex_transparent_multiple_flags() {
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("git --no-pager -C /path log"));
    assert!(re.is_match("git --no-pager -C /path log --oneline"));
}

#[test]
fn regex_transparent_combined_with_basename() {
    // /usr/bin/git -C /path log  →  matches "git log"
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("/usr/bin/git -C /path log"));
    assert!(re.is_match("/usr/bin/git --no-pager -C /path log --oneline"));
}

#[test]
fn regex_non_flag_between_words_no_match() {
    // A non-flag, non-target word between pattern words should NOT match
    let r = command_pattern_to_regex("git log");
    let re = regex::Regex::new(&r).unwrap();
    assert!(
        !re.is_match("git somedir log"),
        "non-flag should block match"
    );
}

#[test]
fn regex_transparent_wildcard_with_flags_before_run() {
    // npm --prefix /path run build  →  matches "npm run *"
    let r = command_pattern_to_regex("npm run *");
    let re = regex::Regex::new(&r).unwrap();
    assert!(re.is_match("npm --prefix /path run build"));
}

#[test]
fn regex_empty_pattern() {
    // Degenerate: empty pattern should produce a valid regex
    let r = command_pattern_to_regex("");
    assert!(regex::Regex::new(&r).is_ok());
}
