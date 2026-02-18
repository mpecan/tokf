#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tokf::config::types::FilterConfig;

/// Helper: build a search dirs list pointing at the real stdlib filters.
fn stdlib_dir() -> Vec<PathBuf> {
    vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters")]
}

#[test]
fn test_resolve_git_push_from_stdlib() {
    let dirs = stdlib_dir();
    let config = tokf::config::resolve_filter_in(&["git", "push"], &dirs)
        .unwrap()
        .unwrap();
    assert_eq!(config.command, "git push");
}

#[test]
fn test_all_stdlib_filters_load() {
    let filters_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters");

    for entry in std::fs::read_dir(&filters_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            let content = std::fs::read_to_string(&path).unwrap();
            let result: Result<FilterConfig, _> = toml::from_str(&content);
            assert!(
                result.is_ok(),
                "Failed to load {}: {:?}",
                path.display(),
                result.err()
            );
        }
    }
}

#[test]
fn test_resolve_filter_public_api() {
    // resolve_filter uses default_search_dirs which includes binary_dir/filters/.
    // In the test harness the binary lives in target/debug/deps/, so it won't find
    // stdlib filters there. But the function should still return Ok (either Some or None)
    // without panicking, proving the full wiring works.
    let result = tokf::config::resolve_filter(&["git", "push"]);
    assert!(result.is_ok());
}

#[test]
fn test_resolve_filter_in_not_found_returns_none() {
    let dirs = stdlib_dir();
    let result =
        tokf::config::resolve_filter_in(&["totally", "nonexistent", "command"], &dirs).unwrap();
    assert!(result.is_none());
}
