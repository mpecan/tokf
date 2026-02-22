#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serial_test::serial;
use tempfile::TempDir;

// --- HistoryConfig ---

#[test]
fn history_config_default_retention() {
    let config = HistoryConfig::default();
    assert_eq!(config.retention_count, 10);
}

#[test]
fn history_config_load_from_project_file() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(tokf_dir.join("config.toml"), "[history]\nretention = 25\n")
        .expect("write config");

    let config = HistoryConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.retention_count, 25);
}

#[test]
fn history_config_load_from_global_config_file() {
    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[history]\nretention = 50\n").expect("write global config");

    let config = HistoryConfig::load_from(None, Some(&global_config));
    assert_eq!(config.retention_count, 50);
}

#[test]
fn history_config_load_project_overrides_global() {
    let project_dir = TempDir::new().expect("tempdir");
    let tokf_dir = project_dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(tokf_dir.join("config.toml"), "[history]\nretention = 15\n")
        .expect("write project config");

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[history]\nretention = 99\n").expect("write global config");

    let config = HistoryConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert_eq!(
        config.retention_count, 15,
        "project config should take priority over global"
    );
}

#[test]
fn history_config_load_falls_back_to_global_when_no_project_config() {
    let project_dir = TempDir::new().expect("tempdir"); // no .tokf/config.toml

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[history]\nretention = 30\n").expect("write global config");

    let config = HistoryConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert_eq!(config.retention_count, 30);
}

#[test]
fn history_config_load_missing_file_falls_back_to_default() {
    let dir = TempDir::new().expect("tempdir");
    // No .tokf/config.toml, no global config
    let config = HistoryConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.retention_count, 10);
}

#[test]
fn history_config_load_malformed_toml_falls_back_to_default() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "this is not valid toml!!!\n[[[",
    )
    .expect("write bad config");

    let config = HistoryConfig::load_from(Some(dir.path()), None);
    assert_eq!(
        config.retention_count, 10,
        "malformed TOML should fall back to default"
    );
}

#[test]
fn history_config_load_malformed_global_config_falls_back_to_default() {
    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[[invalid toml").expect("write bad global config");

    let config = HistoryConfig::load_from(None, Some(&global_config));
    assert_eq!(config.retention_count, 10);
}

// --- current_project ---

#[test]
fn current_project_returns_non_empty_string() {
    let project = current_project();
    assert!(
        !project.is_empty(),
        "current_project() should return a non-empty path string"
    );
}

// --- try_record ---

/// Must run serially: sets `TOKF_DB_PATH` env var.
#[test]
#[serial]
fn try_record_records_entry_to_db() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    unsafe {
        std::env::set_var("TOKF_DB_PATH", db_path.to_str().expect("path str"));
    }

    try_record(
        "git status",
        "git-status",
        "raw output",
        "filtered output",
        0,
    );

    unsafe {
        std::env::remove_var("TOKF_DB_PATH");
    }

    let conn = open_db(&db_path).expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1, "try_record should insert one history entry");
}

/// Must run serially: sets `TOKF_DB_PATH` env var.
#[test]
#[serial]
fn try_record_does_not_panic_on_unwritable_db_path() {
    // Point to a path whose parent cannot be created.
    unsafe {
        std::env::set_var("TOKF_DB_PATH", "/dev/null/no-such-dir/tracking.db");
        std::env::remove_var("TOKF_DEBUG");
    }
    // Must not panic — errors are silently swallowed when TOKF_DEBUG is unset.
    try_record("cmd", "filter", "raw", "filtered", 0);
    unsafe {
        std::env::remove_var("TOKF_DB_PATH");
    }
}

/// Must run serially: sets `TOKF_DB_PATH` and `TOKF_DEBUG` env vars.
#[test]
#[serial]
fn try_record_does_not_panic_on_unwritable_db_path_with_debug() {
    unsafe {
        std::env::set_var("TOKF_DB_PATH", "/dev/null/no-such-dir/tracking.db");
        std::env::set_var("TOKF_DEBUG", "1");
    }
    // Must not panic even when TOKF_DEBUG is set (it logs to stderr but does not panic).
    try_record("cmd", "filter", "raw", "filtered", 0);
    unsafe {
        std::env::remove_var("TOKF_DB_PATH");
        std::env::remove_var("TOKF_DEBUG");
    }
}
