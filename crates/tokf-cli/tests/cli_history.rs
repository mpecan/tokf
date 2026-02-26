#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::items_after_statements
)]

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn tokf_with_db(db_path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tokf"));
    cmd.env("TOKF_DB_PATH", db_path);
    cmd
}

fn temp_db_dir() -> TempDir {
    TempDir::new().expect("tempdir")
}

/// Create a temp directory with a local `.tokf/filters/echo.toml` filter and
/// return `(dir, filters_dir)`. The filter replaces echo output with "filtered".
fn setup_local_filter(show_history_hint: bool) -> TempDir {
    let dir = TempDir::new().unwrap();
    let filters_dir = dir.path().join(".tokf/filters");
    std::fs::create_dir_all(&filters_dir).unwrap();
    let hint_line = if show_history_hint {
        "show_history_hint = true\n"
    } else {
        ""
    };
    std::fs::write(
        filters_dir.join("echo.toml"),
        format!("command = \"echo\"\n{hint_line}[on_success]\noutput = \"filtered\""),
    )
    .unwrap();
    dir
}

// ---------------------------------------------------------------------------
// history show --raw
// ---------------------------------------------------------------------------

#[test]
fn history_show_raw_prints_only_raw_output() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(false);

    // Run a filtered command so history is recorded.
    let run_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "hello world"])
        .output()
        .expect("run");
    assert!(run_out.status.success());

    // List history to find the entry ID.
    let list_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["history", "list"])
        .output()
        .expect("history list");
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    let id: &str = list_stdout.split_whitespace().next().expect("entry ID");

    // Show with --raw: should print only raw output, no metadata.
    let show_out = tokf_with_db(&db)
        .args(["history", "show", "--raw", id])
        .output()
        .expect("history show --raw");
    let stdout = String::from_utf8_lossy(&show_out.stdout);

    assert!(
        show_out.status.success(),
        "exit: {:?}",
        show_out.status.code()
    );
    assert!(
        stdout.contains("hello world"),
        "expected raw output, got: {stdout}"
    );
    // Must NOT contain metadata headers.
    assert!(
        !stdout.contains("ID:"),
        "should not contain metadata, got: {stdout}"
    );
    assert!(
        !stdout.contains("--- Raw Output ---"),
        "should not contain section header, got: {stdout}"
    );
    assert!(
        !stdout.contains("--- Filtered Output ---"),
        "should not contain section header, got: {stdout}"
    );
}

#[test]
fn history_show_default_includes_metadata() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(false);

    tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "hi"])
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

    assert!(show_out.status.success());
    assert!(
        stdout.contains("ID:"),
        "should contain metadata, got: {stdout}"
    );
    assert!(
        stdout.contains("--- Raw Output ---"),
        "should contain raw section, got: {stdout}"
    );
    assert!(
        stdout.contains("--- Filtered Output ---"),
        "should contain filtered section, got: {stdout}"
    );
}

#[test]
fn history_show_not_found_exits_one() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");

    let out = tokf_with_db(&db)
        .args(["history", "show", "99999"])
        .output()
        .expect("history show");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "expected exit 1");
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr, got: {stderr}"
    );
}

#[test]
fn history_show_raw_not_found_exits_one() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");

    let out = tokf_with_db(&db)
        .args(["history", "show", "--raw", "99999"])
        .output()
        .expect("history show --raw");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "expected exit 1");
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// history hint message
// ---------------------------------------------------------------------------

#[test]
fn hint_appears_with_show_history_hint_filter() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(true);

    let out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "test"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success());
    assert!(
        stdout.contains("[tokf] output filtered"),
        "expected hint in stdout, got: {stdout}"
    );
}

#[test]
fn hint_absent_without_show_history_hint() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(false);

    let out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "test"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(out.status.success());
    assert!(
        !stdout.contains("[tokf] output filtered"),
        "hint should not appear, got: {stdout}"
    );
}

#[test]
fn hint_contains_valid_history_id() {
    let db_dir = temp_db_dir();
    let db = db_dir.path().join("tracking.db");
    let work_dir = setup_local_filter(true);

    let run_out = tokf_with_db(&db)
        .current_dir(work_dir.path())
        .args(["run", "echo", "payload"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&run_out.stdout);
    assert!(run_out.status.success());

    // Extract the history ID from the hint line.
    // Format: "[tokf] output filtered — to see what was omitted: `tokf history show --raw 1`"
    let hint_line = stdout
        .lines()
        .find(|l| l.contains("tokf history show --raw"))
        .expect("hint line not found");
    let id = hint_line
        .trim_end_matches('`')
        .rsplit_once(' ')
        .expect("space before ID")
        .1;

    // Verify that the ID is valid by fetching raw output.
    let show_out = tokf_with_db(&db)
        .args(["history", "show", "--raw", id])
        .output()
        .expect("history show --raw");
    let raw_stdout = String::from_utf8_lossy(&show_out.stdout);

    assert!(show_out.status.success(), "show --raw failed for id {id}");
    assert!(
        raw_stdout.contains("payload"),
        "raw output should contain 'payload', got: {raw_stdout}"
    );
}
