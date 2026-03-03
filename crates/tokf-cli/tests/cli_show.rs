#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_const_for_fn)]

use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

// --- tokf show ---

#[test]
fn show_hash_produces_64_char_lowercase_hex() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push", "--hash"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout.trim();
    assert_eq!(hash.len(), 64, "hash must be 64 chars, got: {hash:?}");
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "hash must be lowercase hex, got: {hash:?}"
    );
}

#[test]
fn show_hash_is_stable_across_two_calls() {
    let dir = tempfile::TempDir::new().unwrap();
    let hash1 = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    let hash2 = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_eq!(hash1, hash2, "hash must be identical across invocations");
}

#[test]
fn show_hash_differs_for_different_filters() {
    let dir = tempfile::TempDir::new().unwrap();
    let hash_push = String::from_utf8_lossy(
        &tokf()
            .args(["show", "git/push", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    let hash_build = String::from_utf8_lossy(
        &tokf()
            .args(["show", "cargo/build", "--hash"])
            .current_dir(dir.path())
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();
    assert_ne!(
        hash_push, hash_build,
        "different filters must produce different hashes"
    );
}

#[test]
fn show_hash_nonexistent_filter_exits_one() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "no/such/filter", "--hash"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn show_git_push_prints_toml() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git push"),
        "expected TOML with 'git push' command, got: {stdout}"
    );
    assert!(
        stdout.contains("on_success") || stdout.contains("on_failure"),
        "expected TOML content, got: {stdout}"
    );
}

#[test]
fn show_with_toml_extension_works() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "git/push.toml"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git push"),
        "expected TOML content with .toml extension variant, got: {stdout}"
    );
}

#[test]
fn show_nonexistent_exits_one() {
    let output = tokf().args(["show", "no/such/filter"]).output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("filter not found"),
        "expected 'filter not found' in stderr, got: {stderr}"
    );
}

#[test]
fn show_local_filter_prints_disk_content() {
    let dir = tempfile::TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("my-tool.toml"),
        "command = \"my tool\"\n# local comment\n",
    )
    .unwrap();

    let output = tokf()
        .args(["show", "my-tool"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("local comment"),
        "expected local filter content, got: {stdout}"
    );
}

#[test]
fn show_cargo_build_nested_embedded_path() {
    // Verifies that show works for nested paths (cargo/build) in the embedded stdlib
    let dir = tempfile::TempDir::new().unwrap();
    let output = tokf()
        .args(["show", "cargo/build"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokf show cargo/build should succeed"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo build"),
        "expected TOML with 'cargo build' command, got: {stdout}"
    );
    assert!(
        stdout.contains("on_success") || stdout.contains("skip"),
        "expected TOML content with on_success or skip, got: {stdout}"
    );
}
