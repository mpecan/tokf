//! Integration tests for the rarity-weighted richness metric surfaced by
//! `tokf verify`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::path::Path;
use std::process::Output;

const RICH_INPUT: &str = "Compiling tokf-common v0.1.0\\nthread 'main' panicked at src/lib/module.rs:42:9\\nassertion `leftvalue == rightvalue` failed\\ndeadbeefcafe0123";

/// Build a temp workspace with one synthetic filter and one test case.
fn scaffold(dir: &Path, filter_toml: &str, case_toml: &str) {
    let filters_dir = dir.join("filters").join("mytest");
    fs::create_dir_all(&filters_dir).unwrap();
    let suite_dir = filters_dir.join("cmd_test");
    fs::create_dir_all(&suite_dir).unwrap();
    fs::write(filters_dir.join("cmd.toml"), filter_toml).unwrap();
    fs::write(suite_dir.join("case.toml"), case_toml).unwrap();
}

fn run_verify(dir: &Path, extra: &[&str]) -> Output {
    let home = common::TestHome::new();
    let mut cmd = home.cmd();
    cmd.arg("verify").arg("mytest/cmd");
    cmd.args(extra);
    cmd.current_dir(dir);
    cmd.output().unwrap()
}

/// A passthrough filter keeps everything, so richness is high.
const PASSTHROUGH: &str = "command = \"mytest cmd\"\n";

/// A filter that collapses everything to a fixed string.
const LOSSY: &str = "command = \"mytest cmd\"\n\n[on_success]\noutput = \"OK\"\n";

fn case(extra: &str) -> String {
    format!(
        "name = \"richcase\"\ninline = \"{RICH_INPUT}\"\nexit_code = 0\n{extra}\n[[expect]]\nline_count = 1\n"
    )
}

#[test]
fn verify_prints_richness_per_case() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), LOSSY, &case(""));

    let output = run_verify(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|l| l.contains("richcase"))
        .expect("expected a case line");
    assert!(line.contains("richness"), "got: {line}");
    assert!(
        line.contains("atoms]"),
        "expected kept/atoms counts: {line}"
    );
}

#[test]
fn verify_json_includes_richness_object() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), LOSSY, &case(""));

    let output = run_verify(dir.path(), &["--json"]);
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let richness = &json[0]["cases"][0]["richness"];
    let retained = richness["retained"].as_f64().expect("retained must be f64");
    assert!((0.0..=1.0).contains(&retained), "retained={retained}");
    assert!(richness["atoms"].as_u64().unwrap() > 0);
    assert!(richness["kept"].is_number());
}

#[test]
fn verify_fails_when_min_richness_unmet() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), LOSSY, &case("min_richness = 0.9\n"));

    let output = run_verify(dir.path(), &[]);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("min_richness"), "got:\n{stdout}");
}

#[test]
fn verify_passes_lossy_filter_without_min_richness() {
    // Anti-global-gate regression test: an aggressively lossy filter with no
    // declared threshold must still exit 0.
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), LOSSY, &case(""));

    let output = run_verify(dir.path(), &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn verify_passes_when_min_richness_met() {
    let dir = tempfile::tempdir().unwrap();
    let case_toml = format!(
        "name = \"richcase\"\ninline = \"{RICH_INPUT}\"\nexit_code = 0\nmin_richness = 0.9\n\n[[expect]]\ncontains = \"panicked\"\n"
    );
    scaffold(dir.path(), PASSTHROUGH, &case_toml);

    let output = run_verify(dir.path(), &[]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn verify_rejects_invalid_min_richness_value() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), LOSSY, &case("min_richness = 2.0\n"));

    let output = run_verify(dir.path(), &[]);
    assert_ne!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("min_richness must be between 0.0 and 1.0"),
        "got:\n{stdout}"
    );
}
