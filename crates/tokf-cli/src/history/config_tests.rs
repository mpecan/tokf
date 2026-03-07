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

// --- SyncConfig ---

#[test]
fn sync_config_default() {
    let config = SyncConfig::default();
    assert_eq!(config.auto_sync_threshold, 100);
}

#[test]
fn sync_config_from_toml() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nauto_sync_threshold = 100\n",
    )
    .expect("write config");

    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.auto_sync_threshold, 100);
}

#[test]
fn sync_config_from_global() {
    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[sync]\nauto_sync_threshold = 200\n")
        .expect("write global config");

    let config = SyncConfig::load_from(None, Some(&global_config));
    assert_eq!(config.auto_sync_threshold, 200);
}

#[test]
fn sync_config_project_overrides_global() {
    let project_dir = TempDir::new().expect("tempdir");
    let tokf_dir = project_dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nauto_sync_threshold = 25\n",
    )
    .expect("write project config");

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[sync]\nauto_sync_threshold = 300\n")
        .expect("write global config");

    let config = SyncConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert_eq!(
        config.auto_sync_threshold, 25,
        "project config should take priority over global"
    );
}

#[test]
fn sync_config_falls_back_to_default() {
    let dir = TempDir::new().expect("tempdir");
    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.auto_sync_threshold, 100);
}

#[test]
fn sync_config_zero_disables_auto_sync() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nauto_sync_threshold = 0\n",
    )
    .expect("write config");

    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.auto_sync_threshold, 0);
}

#[test]
fn mixed_history_and_sync_config() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[history]\nretention = 20\n\n[sync]\nauto_sync_threshold = 75\n",
    )
    .expect("write config");

    let history = HistoryConfig::load_from(Some(dir.path()), None);
    let sync = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(history.retention_count, 20);
    assert_eq!(sync.auto_sync_threshold, 75);
}

// --- SyncConfig: upload_usage_stats ---

#[test]
fn sync_config_upload_usage_stats_from_toml() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nupload_usage_stats = true\n",
    )
    .expect("write config");

    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.upload_usage_stats, Some(true));
}

#[test]
fn sync_config_upload_usage_stats_false() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nupload_usage_stats = false\n",
    )
    .expect("write config");

    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.upload_usage_stats, Some(false));
}

#[test]
fn sync_config_upload_usage_stats_default_is_none() {
    let dir = TempDir::new().expect("tempdir");
    let config = SyncConfig::load_from(Some(dir.path()), None);
    assert_eq!(config.upload_usage_stats, None);
}

#[test]
fn sync_config_upload_usage_stats_from_global() {
    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[sync]\nupload_usage_stats = true\n")
        .expect("write global config");

    let config = SyncConfig::load_from(None, Some(&global_config));
    assert_eq!(config.upload_usage_stats, Some(true));
}

#[test]
fn sync_config_upload_usage_stats_project_overrides_global() {
    let project_dir = TempDir::new().expect("tempdir");
    let tokf_dir = project_dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nupload_usage_stats = false\n",
    )
    .expect("write project config");

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[sync]\nupload_usage_stats = true\n")
        .expect("write global config");

    let config = SyncConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert_eq!(
        config.upload_usage_stats,
        Some(false),
        "project config should take priority over global"
    );
}

// --- SyncConfig: partial [sync] section fallthrough ---

#[test]
fn sync_config_partial_section_falls_through_to_global() {
    // Project has [sync] with only auto_sync_threshold, no upload_usage_stats.
    // Global has [sync] with only upload_usage_stats.
    // Result: threshold from project, upload_usage_stats from global.
    let project_dir = TempDir::new().expect("tempdir");
    let tokf_dir = project_dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(
        tokf_dir.join("config.toml"),
        "[sync]\nauto_sync_threshold = 42\n",
    )
    .expect("write project config");

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[sync]\nupload_usage_stats = true\n")
        .expect("write global config");

    let config = SyncConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert_eq!(
        config.auto_sync_threshold, 42,
        "threshold should come from project"
    );
    assert_eq!(
        config.upload_usage_stats,
        Some(true),
        "upload_usage_stats should fall through to global"
    );
}

// --- ShimsConfig ---

#[test]
fn shims_config_default_enabled() {
    let config = ShimsConfig::default();
    assert!(config.enabled);
}

#[test]
fn shims_config_load_from_project_file() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(tokf_dir.join("config.toml"), "[shims]\nenabled = false\n")
        .expect("write config");

    let config = ShimsConfig::load_from(Some(dir.path()), None);
    assert!(!config.enabled);
}

#[test]
fn shims_config_load_from_global_config_file() {
    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[shims]\nenabled = false\n").expect("write global config");

    let config = ShimsConfig::load_from(None, Some(&global_config));
    assert!(!config.enabled);
}

#[test]
fn shims_config_project_overrides_global() {
    let project_dir = TempDir::new().expect("tempdir");
    let tokf_dir = project_dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(tokf_dir.join("config.toml"), "[shims]\nenabled = false\n")
        .expect("write project config");

    let global_dir = TempDir::new().expect("tempdir");
    let global_config = global_dir.path().join("config.toml");
    std::fs::write(&global_config, "[shims]\nenabled = true\n").expect("write global config");

    let config = ShimsConfig::load_from(Some(project_dir.path()), Some(&global_config));
    assert!(
        !config.enabled,
        "project config should take priority over global"
    );
}

#[test]
fn shims_config_falls_back_to_default() {
    let dir = TempDir::new().expect("tempdir");
    let config = ShimsConfig::load_from(Some(dir.path()), None);
    assert!(config.enabled, "default should be true");
}

#[test]
fn shims_config_malformed_toml_falls_back_to_default() {
    let dir = TempDir::new().expect("tempdir");
    let tokf_dir = dir.path().join(".tokf");
    std::fs::create_dir(&tokf_dir).expect("create .tokf");
    std::fs::write(tokf_dir.join("config.toml"), "not valid toml!!!\n[[[")
        .expect("write bad config");

    let config = ShimsConfig::load_from(Some(dir.path()), None);
    assert!(
        config.enabled,
        "malformed TOML should fall back to default (true)"
    );
}

// --- save_upload_stats_to_path ---

#[test]
fn save_upload_stats_to_path_creates_file() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.toml");

    config::save_upload_stats_to_path(&path, true).expect("save");

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(
        content.contains("upload_usage_stats = true"),
        "got: {content}"
    );
}

#[test]
fn save_upload_stats_to_path_preserves_other_fields() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[history]\nretention = 25\n\n[sync]\nauto_sync_threshold = 50\n",
    )
    .expect("write");

    config::save_upload_stats_to_path(&path, false).expect("save");

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(
        content.contains("retention = 25"),
        "retention preserved: {content}"
    );
    assert!(
        content.contains("auto_sync_threshold = 50"),
        "threshold preserved: {content}"
    );
    assert!(
        content.contains("upload_usage_stats = false"),
        "stats set: {content}"
    );
}

#[test]
fn save_upload_stats_to_path_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.toml");

    config::save_upload_stats_to_path(&path, true).expect("save");

    let config = SyncConfig::load_from(None, Some(&path));
    assert_eq!(config.upload_usage_stats, Some(true));
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

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_record_records_entry_to_db() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    let _guard = crate::paths::DbPathGuard::set(db_path.clone());

    let _ = try_record(
        "git status",
        "git-status",
        "raw output",
        "filtered output",
        0,
    );

    let conn = open_db(&db_path).expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 1, "try_record should insert one history entry");
}

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_record_returns_id_on_success() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    let _guard = crate::paths::DbPathGuard::set(db_path);

    let id = try_record("cargo test", "cargo/test", "raw", "filtered", 0);

    assert!(id.is_some(), "try_record should return Some(id) on success");
    assert_eq!(id.unwrap(), 1, "first inserted entry should have id=1");
}

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_record_does_not_panic_on_unwritable_db_path() {
    // Point to a path whose parent cannot be created.
    let _db_guard = crate::paths::DbPathGuard::set("/dev/null/no-such-dir/tracking.db");
    let _debug_guard = crate::paths::DebugGuard::new(false);
    // Must not panic — errors are silently swallowed when TOKF_DEBUG is unset.
    let result = try_record("cmd", "filter", "raw", "filtered", 0);
    assert!(
        result.is_none(),
        "try_record should return None on db error"
    );
}

/// Must run serially: sets `TOKF_DB_PATH` and `TOKF_DEBUG` overrides.
#[test]
#[serial]
fn try_record_does_not_panic_on_unwritable_db_path_with_debug() {
    let _db_guard = crate::paths::DbPathGuard::set("/dev/null/no-such-dir/tracking.db");
    let _debug_guard = crate::paths::DebugGuard::new(true);
    // Must not panic even when TOKF_DEBUG is set (it logs to stderr but does not panic).
    let _ = try_record("cmd", "filter", "raw", "filtered", 0);
}

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_was_recently_run_returns_true_for_repeated_command() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    let _guard = crate::paths::DbPathGuard::set(db_path);

    // Record first run.
    let _ = try_record("git status", "git/status", "raw", "filtered", 0);

    // Now the same command was "recently run".
    let repeated = try_was_recently_run("git status");

    assert!(repeated, "same command should be detected as recently run");
}

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_was_recently_run_returns_false_for_different_command() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    let _guard = crate::paths::DbPathGuard::set(db_path);

    let _ = try_record("git status", "git/status", "raw", "filtered", 0);
    let repeated = try_was_recently_run("cargo test");

    assert!(
        !repeated,
        "different command should not be detected as recently run"
    );
}

/// Must run serially: sets `TOKF_DB_PATH` override.
#[test]
#[serial]
fn try_was_recently_run_returns_false_on_empty_history() {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("tracking.db");
    let _guard = crate::paths::DbPathGuard::set(db_path);

    // No entries recorded yet.
    let repeated = try_was_recently_run("git status");

    assert!(!repeated, "empty history should return false");
}
