//! End-to-end coverage for `run`-override transparency (issue #430).
//!
//! A filter's `run` field makes tokf execute a different command than the user
//! typed. These tests pin the three surfaces that disclose it — the `Executed:`
//! line, the `tokf raw` stderr note, and the `--verbose` message — and, just as
//! importantly, that `tokf raw`'s stdout stays free of them so pipes still work.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn tokf_with_db(db_path: &Path) -> Command {
    // TOKF_HOME points at a directory that does not exist, so the binary never
    // finds a real auth.toml and never touches the OS keyring during tests.
    let mut cmd = common::isolated_command(&db_path.parent().unwrap().join("tokf-home"));
    cmd.env("TOKF_DB_PATH", db_path);
    cmd
}

fn temp_db_dir() -> TempDir {
    TempDir::new().expect("tempdir")
}

/// A local filter that runs the user's command verbatim — the control case.
fn setup_local_filter(_show_history_hint: bool) -> TempDir {
    let dir = TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("echo.toml"),
        "command = \"echo\"\n[on_success]\noutput = \"filtered\"",
    )
    .unwrap();
    dir
}

/// Local filter whose `run` field substitutes a different command than the one
/// the user types — the case issue #430 is about.
fn setup_substituting_filter() -> TempDir {
    let dir = TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    std::fs::write(
        filters_dir.join("echo.toml"),
        "command = \"echo\"\nrun = \"echo substituted-output {args}\"\n\
         [on_success]\noutput = \"filtered\"",
    )
    .unwrap();
    dir
}

/// Run the substituting filter once and return `(db_dir, work_dir, entry_id)`.
fn record_substituted_run() -> (TempDir, TempDir, String) {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_substituting_filter();

    let run_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "typed-by-user"])
        .output()
        .expect("run");
    assert!(run_out.status.success());

    let list_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["history", "list"])
        .output()
        .expect("history list");
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    let id = list_stdout
        .split_whitespace()
        .next()
        .expect("entry ID")
        .to_string();

    (db_dir, work_dir, id)
}

#[test]
fn history_show_reports_the_substituted_command() {
    let (db_dir, _work_dir, id) = record_substituted_run();
    let db = db_dir.path().join("tracking.db");

    let show_out = tokf_with_db(&db)
        .args(["history", "show", &id])
        .output()
        .expect("history show");
    let stdout = String::from_utf8_lossy(&show_out.stdout);

    assert!(
        stdout.contains("Command: echo typed-by-user"),
        "the user's command must still be recorded, got: {stdout}"
    );
    assert!(
        stdout.contains("Executed: echo substituted-output"),
        "the substituted command must be shown, got: {stdout}"
    );
    // The captured output is the substitute's, and the entry now says so.
    assert!(
        stdout.contains("substituted-output"),
        "raw output should be the substitute's, got: {stdout}"
    );
}

#[test]
fn history_show_omits_executed_line_without_a_run_override() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(false);

    tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "hello"])
        .output()
        .expect("run");
    let list_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["history", "list"])
        .output()
        .expect("history list");
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    let id = list_stdout.split_whitespace().next().expect("entry ID");

    let show_out = tokf_with_db(&db)
        .args(["history", "show", id])
        .output()
        .expect("history show");
    let stdout = String::from_utf8_lossy(&show_out.stdout);

    assert!(stdout.contains("Command: echo hello"));
    assert!(
        !stdout.contains("Executed:"),
        "a verbatim run must not claim a substitution, got: {stdout}"
    );
}

#[test]
fn raw_note_goes_to_stderr_leaving_stdout_pure() {
    let (db_dir, _work_dir, id) = record_substituted_run();
    let db = db_dir.path().join("tracking.db");

    let out = tokf_with_db(&db)
        .args(["raw", &id])
        .output()
        .expect("tokf raw");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // The whole point of the stderr split: `tokf raw <id> | grep …` must see
    // the recovered output and nothing else.
    assert!(
        stdout.contains("substituted-output"),
        "stdout must carry the raw output, got: {stdout}"
    );
    assert!(
        !stdout.contains("[tokf]"),
        "stdout must stay pure — no note, got: {stdout}"
    );
    assert!(
        stderr.contains("this output came from"),
        "the substitution note must be on stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("echo substituted-output"),
        "the note must name the executed command, got: {stderr}"
    );
}

#[test]
fn verbose_run_reports_the_substitution_on_stderr() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_substituting_filter();

    let out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "--verbose", "echo", "typed-by-user"])
        .output()
        .expect("run --verbose");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("[tokf] executing: echo substituted-output"),
        "--verbose must report what actually ran, got: {stderr}"
    );
    assert!(
        stderr.contains("echo typed-by-user"),
        "--verbose must name the command it was substituted for, got: {stderr}"
    );
}
