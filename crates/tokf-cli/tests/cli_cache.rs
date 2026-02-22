use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// Create a temp dir with `.tokf/` so the cache path is predictable.
fn setup_project_dir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join(".tokf")).unwrap();
    tmp
}

fn cache_path(project_dir: &TempDir) -> PathBuf {
    project_dir.path().join(".tokf/cache/manifest.bin")
}

#[test]
fn cache_clear_exits_zero_when_no_cache() {
    let tmp = setup_project_dir();
    let output = tokf()
        .current_dir(tmp.path())
        .args(["cache", "clear"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cache_info_shows_path() {
    let tmp = setup_project_dir();
    let output = tokf()
        .current_dir(tmp.path())
        .args(["cache", "info"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cache path:"),
        "expected 'cache path:' in output, got: {stdout}"
    );
    assert!(
        stdout.contains(".tokf"),
        "expected '.tokf' in path output, got: {stdout}"
    );
}

#[test]
fn cache_populated_after_run() {
    let tmp = setup_project_dir();
    let cache = cache_path(&tmp);
    assert!(!cache.exists(), "cache should not exist before first run");

    let output = tokf()
        .current_dir(tmp.path())
        .args(["run", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());

    assert!(
        cache.exists(),
        "cache should exist after first run at {}",
        cache.display()
    );

    // Also verify cache info reports it as valid
    let info = tokf()
        .current_dir(tmp.path())
        .args(["cache", "info"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(
        stdout.contains("valid: true"),
        "expected valid cache, got: {stdout}"
    );
}

#[test]
fn cache_clear_removes_file() {
    let tmp = setup_project_dir();
    let cache = cache_path(&tmp);

    // Populate cache
    tokf()
        .current_dir(tmp.path())
        .args(["run", "echo", "hello"])
        .output()
        .unwrap();
    assert!(cache.exists());

    // Clear it
    let output = tokf()
        .current_dir(tmp.path())
        .args(["cache", "clear"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!cache.exists(), "cache should be gone after clear");
}

#[test]
fn no_cache_flag_skips_writing() {
    let tmp = setup_project_dir();
    let cache = cache_path(&tmp);

    let output = tokf()
        .current_dir(tmp.path())
        .args(["run", "--no-cache", "echo", "hello"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !cache.exists(),
        "--no-cache should not write cache at {}",
        cache.display()
    );
}

#[test]
fn second_run_hits_cache() {
    let tmp = setup_project_dir();
    let cache = cache_path(&tmp);

    // First run: populate cache
    tokf()
        .current_dir(tmp.path())
        .args(["run", "echo", "hello"])
        .output()
        .unwrap();
    assert!(cache.exists());

    let mtime_after_first = fs::metadata(&cache).unwrap().modified().unwrap();

    // Brief pause to allow mtime to differ if the file were rewritten
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Second run: should hit cache (no write)
    tokf()
        .current_dir(tmp.path())
        .args(["run", "echo", "hello"])
        .output()
        .unwrap();

    let mtime_after_second = fs::metadata(&cache).unwrap().modified().unwrap();
    assert_eq!(
        mtime_after_first, mtime_after_second,
        "cache file should not be rewritten on cache hit"
    );
}
