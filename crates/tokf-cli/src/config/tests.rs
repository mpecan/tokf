#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::needless_collect
)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;

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
            // Skip test case files: those living inside a <stem>_test/ suite directory.
            if path
                .components()
                .any(|c| c.as_os_str().to_string_lossy().ends_with("_test"))
            {
                continue;
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
