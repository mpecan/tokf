#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::needless_collect
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::*;

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
