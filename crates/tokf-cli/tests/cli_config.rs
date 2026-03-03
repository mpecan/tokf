#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

#[test]
fn config_path_shows_locations() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "path"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("global"),
        "expected 'global' in output:\n{stdout}"
    );
    assert!(
        stdout.contains("local"),
        "expected 'local' in output:\n{stdout}"
    );
}

#[test]
fn config_path_with_tokf_home() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("custom_home");
    std::fs::create_dir_all(&home).unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "path"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains(home.to_str().unwrap()),
        "expected TOKF_HOME path in output:\n{stdout}"
    );
}

#[test]
fn config_show_defaults() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "show"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("history.retention"),
        "expected history.retention in output:\n{stdout}"
    );
    assert!(
        stdout.contains("sync.auto_sync_threshold"),
        "expected sync.auto_sync_threshold in output:\n{stdout}"
    );
    assert!(
        stdout.contains("sync.upload_stats"),
        "expected sync.upload_stats in output:\n{stdout}"
    );
    assert!(
        stdout.contains("(default)"),
        "expected default source marker:\n{stdout}"
    );
}

#[test]
fn config_show_json_valid() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "show", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("config show --json should be valid JSON");
    assert!(parsed.is_array(), "expected JSON array, got: {parsed}");
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 3, "expected 3 config entries");
    assert!(arr[0]["key"].is_string());
    assert!(arr[0]["value"].is_string());
    assert!(arr[0]["source"].is_string());
}

#[test]
fn config_set_and_get_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Set
    let set_output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "set", "history.retention", "42"])
        .output()
        .unwrap();
    assert!(
        set_output.status.success(),
        "set stderr: {}",
        String::from_utf8_lossy(&set_output.stderr)
    );

    // Get
    let get_output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "get", "history.retention"])
        .output()
        .unwrap();
    assert!(get_output.status.success());
    let stdout = String::from_utf8_lossy(&get_output.stdout);
    assert_eq!(stdout.trim(), "42", "expected 42, got: {stdout}");
}

#[test]
fn config_set_local_flag() {
    let tmp = TempDir::new().unwrap();
    // Create .tokf dir to simulate project root
    std::fs::create_dir_all(tmp.path().join(".tokf")).unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().join("global").to_str().unwrap())
        .args(["config", "set", "history.retention", "15", "--local"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify it was written to local config
    let local_config = tmp.path().join(".tokf/config.toml");
    assert!(
        local_config.exists(),
        "expected local config to be created at {}",
        local_config.display()
    );
    let content = std::fs::read_to_string(&local_config).unwrap();
    assert!(
        content.contains("retention = 15"),
        "expected retention in local config:\n{content}"
    );
}

#[test]
fn config_set_unknown_key_fails() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "set", "nonexistent.key", "value"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "expected failure for unknown key");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown config key"),
        "expected error message:\n{stderr}"
    );
}

#[test]
fn config_set_invalid_value_fails() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "set", "history.retention", "not_a_number"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure for invalid value"
    );
}

#[test]
fn config_print_shows_raw_content() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let config_content = "[history]\nretention = 99\n";
    std::fs::write(home.join("config.toml"), config_content).unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "print", "--global"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.as_ref(),
        config_content,
        "expected raw config content"
    );
}

#[test]
fn config_get_unknown_key_fails() {
    let tmp = TempDir::new().unwrap();
    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", tmp.path().to_str().unwrap())
        .args(["config", "get", "nonexistent.key"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure for unknown key on get"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown config key"),
        "expected error message:\n{stderr}"
    );
}

#[test]
fn config_print_missing_file_fails() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("empty_home");
    std::fs::create_dir_all(&home).unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "print", "--global"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected failure when config file does not exist"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "expected 'not found' error:\n{stderr}"
    );
}

#[test]
fn config_show_local_overrides_global() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Set global value
    std::fs::write(home.join("config.toml"), "[history]\nretention = 50\n").unwrap();

    // Set local value (takes priority)
    std::fs::create_dir_all(tmp.path().join(".tokf")).unwrap();
    std::fs::write(
        tmp.path().join(".tokf/config.toml"),
        "[history]\nretention = 5\n",
    )
    .unwrap();

    let output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Value should be 5 (local), not 50 (global)
    assert!(
        stdout.contains("history.retention = 5"),
        "expected local value 5:\n{stdout}"
    );
    assert!(
        stdout.contains("(local:"),
        "expected local source marker:\n{stdout}"
    );
}

#[test]
fn config_set_upload_stats_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Set upload_stats
    let set_output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "set", "sync.upload_stats", "true"])
        .output()
        .unwrap();
    assert!(set_output.status.success());

    // Get upload_stats
    let get_output = tokf()
        .current_dir(tmp.path())
        .env("TOKF_HOME", home.to_str().unwrap())
        .args(["config", "get", "sync.upload_stats"])
        .output()
        .unwrap();
    assert!(get_output.status.success());
    let stdout = String::from_utf8_lossy(&get_output.stdout);
    assert_eq!(stdout.trim(), "true");
}
