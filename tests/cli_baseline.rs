use std::process::Command;

fn tokf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tokf"))
}

/// Helper: produce deterministic 10-line output for baseline tests.
const TEN_LINE_CMD: &str = "for i in 1 2 3 4 5 6 7 8 9 10; do echo line$i; done";

/// Helper: query the last event from the tracking DB.
fn last_event(db_path: &std::path::Path) -> (i64, i64) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT input_bytes, output_bytes FROM events ORDER BY rowid DESC LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

// --- Core baseline-pipe tracking ---

/// With --baseline-pipe 'tail -3', input_bytes should reflect ~3 lines, not all 10.
#[test]
fn baseline_pipe_records_piped_byte_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-fair.db");

    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "tail -3",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokf run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let (input_bytes, _) = last_event(&db_path);

    // `tail -3` of 10 lines → ~18-20 bytes. Must be much less than full (~61 bytes).
    assert!(
        input_bytes > 0,
        "input_bytes should be positive, got {input_bytes}"
    );
    assert!(
        input_bytes < 30,
        "input_bytes should reflect ~3 lines (~18 bytes), got {input_bytes}"
    );
}

/// Without --baseline-pipe, input_bytes should be the full output length.
#[test]
fn no_baseline_pipe_records_full_byte_count() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-full.db");

    let output = tokf()
        .args(["run", "--no-filter", "sh", "-c", TEN_LINE_CMD])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let (input_bytes, _) = last_event(&db_path);

    // Full output: "line1\nline2\n...line10\n" ≈ 61 bytes
    assert!(
        input_bytes > 50,
        "input_bytes should reflect full output (~61 bytes), got {input_bytes}"
    );
}

// --- Passthrough output_bytes fix (remediation #2) ---

/// In the no-filter passthrough path, output_bytes should be the full raw output
/// (what tokf actually printed), not the piped baseline.
#[test]
fn passthrough_output_bytes_is_full_output() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-passthrough.db");

    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "tail -3",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let (input_bytes, output_bytes) = last_event(&db_path);

    // input_bytes = piped baseline (~18 bytes), output_bytes = full raw (~61 bytes)
    assert!(
        input_bytes < 30,
        "input_bytes should be piped baseline, got {input_bytes}"
    );
    assert!(
        output_bytes > 50,
        "output_bytes should be full output, got {output_bytes}"
    );
    assert!(
        output_bytes > input_bytes,
        "output_bytes ({output_bytes}) should exceed input_bytes ({input_bytes})"
    );
}

// --- Pipe command validation (remediation #5) ---

/// An unrecognised pipe command is rejected and falls back to full output.
#[test]
fn baseline_pipe_rejects_unknown_command() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-reject.db");

    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "wc -l",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Should have warned on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not allowed"),
        "expected rejection warning, got: {stderr}"
    );

    // Should fall back to full output size
    let (input_bytes, _) = last_event(&db_path);
    assert!(
        input_bytes > 50,
        "should fall back to full output, got {input_bytes}"
    );
}

// --- Pipe command failure (remediation #3) ---

/// When the pipe command fails (e.g. grep with bad args producing no output),
/// tokf does not crash and still records a tracking event.
#[test]
fn baseline_pipe_does_not_crash_on_pipe_failure() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-fail.db");

    // grep with --nonexistent-flag exits with error and produces no stdout.
    // compute_baseline will return 0 (stdout was empty). The important thing
    // is that tokf doesn't crash and the event is recorded.
    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "grep --nonexistent-flag-xyz",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "tokf should not crash on pipe failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Tracking should still have recorded an event (input_bytes may be 0
    // since the failing grep produced no stdout).
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "should record exactly one event");
}

// --- Grep baseline ---

/// baseline-pipe with grep records byte count of matching lines only.
#[test]
fn baseline_pipe_grep_records_matching_lines() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-grep.db");

    // Only "line1" and "line10" match "line1" → ~12 bytes (line1\nline10\n)
    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "grep line1",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let (input_bytes, _) = last_event(&db_path);

    // "line1\nline10\n" = 12 bytes. Must be much less than full (~61 bytes).
    assert!(
        input_bytes > 0,
        "input_bytes should be positive, got {input_bytes}"
    );
    assert!(
        input_bytes < 20,
        "input_bytes should reflect matching lines (~12 bytes), got {input_bytes}"
    );
}

// --- Empty output ---

/// baseline-pipe with a command that produces no output records input_bytes = 0.
#[test]
fn baseline_pipe_empty_output() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-empty.db");

    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "tail -3",
            "sh",
            "-c",
            "exit 0",
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let (input_bytes, output_bytes) = last_event(&db_path);
    assert_eq!(input_bytes, 0, "empty command → input_bytes = 0");
    assert_eq!(output_bytes, 0, "empty command → output_bytes = 0");
}

// --- Head baseline ---

/// baseline-pipe with head records byte count of first N lines.
#[test]
fn baseline_pipe_head_records_first_lines() {
    let dir = tempfile::TempDir::new().unwrap();
    let db_path = dir.path().join("test-head.db");

    let output = tokf()
        .args([
            "run",
            "--no-filter",
            "--baseline-pipe",
            "head -2",
            "sh",
            "-c",
            TEN_LINE_CMD,
        ])
        .env("TOKF_DB_PATH", &db_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let (input_bytes, _) = last_event(&db_path);

    // "line1\nline2\n" = 12 bytes
    assert!(
        input_bytes > 0 && input_bytes < 20,
        "input_bytes should reflect ~2 lines (~12 bytes), got {input_bytes}"
    );
}
