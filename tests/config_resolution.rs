#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use tokf::config;

/// Helper: stdlib filters directory.
fn stdlib_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters")
}

// --- discover_all_filters with stdlib ---

#[test]
fn test_discover_git_push_from_stdlib() {
    let dirs = vec![stdlib_dir()];
    let filters = config::discover_all_filters(&dirs).unwrap();
    let git_push = filters
        .iter()
        .find(|f| f.config.command.first() == "git push")
        .expect("git push filter not found");
    assert_eq!(git_push.config.command.first(), "git push");
}

#[test]
fn test_all_stdlib_filters_load() {
    let dirs = vec![stdlib_dir()];
    let filters = config::discover_all_filters(&dirs).unwrap();
    // All 7 stdlib filters should load
    assert_eq!(
        filters.len(),
        7,
        "expected 7 stdlib filters, got {}",
        filters.len()
    );
}

#[test]
fn test_discover_returns_ok_for_nonexistent_dir() {
    let dirs = vec![PathBuf::from("/no/such/directory/ever")];
    let result = config::discover_all_filters(&dirs);
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_discover_nonexistent_command_returns_none() {
    let dirs = vec![stdlib_dir()];
    let filters = config::discover_all_filters(&dirs).unwrap();
    let words = ["totally", "nonexistent", "command"];
    let found = filters.iter().any(|f| f.matches(&words).is_some());
    assert!(!found);
}

// --- CommandPattern matching ---

#[test]
fn test_single_pattern_match() {
    let dirs = vec![stdlib_dir()];
    let filters = config::discover_all_filters(&dirs).unwrap();
    let git_push = filters
        .iter()
        .find(|f| f.config.command.first() == "git push")
        .unwrap();

    // Exact match
    assert_eq!(git_push.matches(&["git", "push"]), Some(2));
    // With trailing args
    assert_eq!(
        git_push.matches(&["git", "push", "origin", "main"]),
        Some(2)
    );
    // No match
    assert_eq!(git_push.matches(&["git", "status"]), None);
}

#[test]
fn test_multiple_pattern_match() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("test-runner.toml"),
        r#"command = ["pnpm test", "npm test"]"#,
    )
    .unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = config::discover_all_filters(&dirs).unwrap();
    assert_eq!(filters.len(), 1);

    assert_eq!(filters[0].matches(&["pnpm", "test"]), Some(2));
    assert_eq!(filters[0].matches(&["npm", "test"]), Some(2));
    assert_eq!(filters[0].matches(&["yarn", "test"]), None);
}

#[test]
fn test_wildcard_pattern_match() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("npm-run.toml"), r#"command = "npm run *""#).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = config::discover_all_filters(&dirs).unwrap();

    assert_eq!(filters[0].matches(&["npm", "run", "build"]), Some(3));
    assert_eq!(filters[0].matches(&["npm", "run", "test"]), Some(3));
    // Wildcard requires at least one token after "run"
    assert_eq!(filters[0].matches(&["npm", "run"]), None);
}

// --- Priority and dedup ---

#[test]
fn test_priority_first_match_wins() {
    use tempfile::TempDir;

    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();

    // Same command string in both dirs
    fs::write(dir1.path().join("a.toml"), r#"command = "git push""#).unwrap();
    fs::write(dir2.path().join("b.toml"), r#"command = "git push""#).unwrap();

    let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
    let filters = config::discover_all_filters(&dirs).unwrap();

    // Dedup: only one entry for "git push"
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].priority, 0); // from dir1 (priority 0)
}

#[test]
fn test_specificity_ordering() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.toml"), r#"command = "git *""#).unwrap();
    fs::write(dir.path().join("b.toml"), r#"command = "git push""#).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = config::discover_all_filters(&dirs).unwrap();

    // "git push" (specificity 2) before "git *" (specificity 1)
    assert_eq!(filters[0].config.command.first(), "git push");
    assert_eq!(filters[1].config.command.first(), "git *");
}

// --- Nested directory discovery ---

#[test]
fn test_nested_dir_discovery() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let git_dir = dir.path().join("git");
    fs::create_dir_all(&git_dir).unwrap();
    fs::write(git_dir.join("push.toml"), r#"command = "git push""#).unwrap();
    fs::write(git_dir.join("status.toml"), r#"command = "git status""#).unwrap();

    let dirs = vec![dir.path().to_path_buf()];
    let filters = config::discover_all_filters(&dirs).unwrap();

    assert_eq!(filters.len(), 2);
    let commands: Vec<&str> = filters.iter().map(|f| f.config.command.first()).collect();
    assert!(commands.contains(&"git push"));
    assert!(commands.contains(&"git status"));
}

// --- pattern_matches_prefix ---

#[test]
fn test_pattern_matches_prefix_basic() {
    assert_eq!(
        config::pattern_matches_prefix("git push", &["git", "push"]),
        Some(2)
    );
    assert_eq!(
        config::pattern_matches_prefix("git push", &["git", "push", "origin"]),
        Some(2)
    );
    assert_eq!(
        config::pattern_matches_prefix("git push", &["git", "status"]),
        None
    );
}

#[test]
fn test_pattern_specificity_ordering() {
    assert!(config::pattern_specificity("git push") > config::pattern_specificity("git *"));
    assert!(config::pattern_specificity("git *") == config::pattern_specificity("* push"));
    assert_eq!(config::pattern_specificity("git push"), 2);
    assert_eq!(config::pattern_specificity("git *"), 1);
}

// --- try_load_filter still works ---

#[test]
fn test_try_load_filter_stdlib() {
    let path = stdlib_dir().join("git/push.toml");
    let config = tokf::config::try_load_filter(&path).unwrap().unwrap();
    assert_eq!(config.command.first(), "git push");
}
