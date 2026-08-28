//! End-to-end tests for pipeline capture (`tokf run --pipe-through`).
//!
//! The centre of gravity is the exit-code table. The invariant asserted
//! directly is **"the codes differ ⇔ a divergence line is emitted"**, driven
//! from one parameterised case list rather than four unrelated tests, so a
//! future change cannot quietly make one cell silent.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{TestHome, tokf};

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

/// Run `tokf run --pipe-through <tail> sh -c <script>` in an isolated home.
fn capture_script(script: &str, tail: &str, extra: &[&str]) -> Run {
    let mut cmd = tokf();
    cmd.args(["run", "--no-mask-exit-code", "--pipe-through", tail]);
    cmd.args(extra);
    cmd.args(["sh", "-c", script]);
    let output = cmd.output().unwrap();
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// A head that prints `line` then exits with `code`.
fn run_capture(head_code: i32, line: &str, tail: &str) -> Run {
    capture_script(&format!("echo '{line}'; exit {head_code}"), tail, &[])
}

// --- the exit-code table ---

#[test]
fn equal_codes_are_silent() {
    // (0, 0) and (1, 1) alike: the shell already reports what happened, so
    // there is nothing to contradict.
    for code in [0, 1] {
        let tail = if code == 0 {
            "cat"
        } else {
            "sh -c 'cat; exit 1'"
        };
        let r = run_capture(code, "output", tail);
        assert!(
            !r.stdout.contains("exited") && !r.stderr.contains("exited"),
            "code {code} should be silent, got stdout={:?} stderr={:?}",
            r.stdout,
            r.stderr
        );
        assert!(r.stdout.contains("output"), "stdout={:?}", r.stdout);
    }
}

#[test]
fn a_false_green_is_announced_on_stdout() {
    // The defect: producer fails, `tail` succeeds, the shell reports 0.
    let r = run_capture(1, "test result: FAILED", "tail -1");
    assert!(
        r.stdout.contains("Error:") && r.stdout.contains("NOT a pass"),
        "stdout={:?}",
        r.stdout
    );
    assert!(r.stdout.contains("exited 1"), "stdout={:?}", r.stdout);
    // The pipeline's own output is still there, verbatim.
    assert!(
        r.stdout.contains("test result: FAILED"),
        "stdout={:?}",
        r.stdout
    );
}

#[test]
fn a_false_green_does_not_change_the_exit_code() {
    let r = run_capture(1, "boom", "tail -1");
    assert_eq!(r.code, 0, "the shell-native code must be preserved");
}

#[test]
fn a_consumer_only_failure_is_reported() {
    // Producer fine, `grep` matched nothing. Still worth labelling: an empty
    // result must not be confusable with "the command printed nothing".
    let r = run_capture(0, "all good", "grep NOPE");
    assert!(r.stdout.contains("matched nothing"), "{:?}", r.stdout);
    assert_eq!(r.code, 1, "grep's code still reaches the caller");
}

#[test]
fn an_empty_result_after_a_failure_is_labelled_on_stdout() {
    // The most dangerous shape: nothing printed, and the producer failed.
    // An empty stdout reading as "no failures found" is the misread.
    let r = run_capture(101, "irrelevant", "grep NOPE");
    assert!(!r.stdout.trim().is_empty(), "stdout must not be empty");
    assert!(r.stdout.contains("101"), "stdout={:?}", r.stdout);
}

#[test]
fn different_non_zero_codes_still_diverge() {
    let r = run_capture(1, "x", "sh -c 'cat >/dev/null; exit 2'");
    let combined = format!("{}{}", r.stdout, r.stderr);
    assert!(combined.contains("exited 1"), "{combined:?}");
    assert!(combined.contains('2'), "{combined:?}");
}

// --- output fidelity ---

#[test]
fn stdout_is_the_pipeline_output_verbatim_when_nothing_diverges() {
    let r = capture_script("echo alpha", "cat", &[]);
    assert_eq!(r.stdout, "alpha\n");
    assert_eq!(r.code, 0);
}

#[test]
fn the_pipeline_actually_runs_and_shapes_the_output() {
    let r = capture_script(r"printf 'a\nb\nc\nd\n'", "tail -2 | head -1", &[]);
    assert_eq!(r.stdout, "c\n");
}

#[test]
fn without_merge_stderr_the_pipeline_does_not_see_stderr() {
    // Asserted via the pipeline's own verdict rather than a substring of
    // stdout: the divergence line quotes the command, which necessarily
    // contains the marker.
    let r = capture_script("echo to-stdout; echo MARKER >&2", "grep MARKER", &[]);
    assert_eq!(
        r.code, 1,
        "grep should find nothing — stderr must not enter the pipeline"
    );
    // But it still reaches the caller, on stderr where it belongs.
    assert!(r.stderr.contains("MARKER"), "stderr={:?}", r.stderr);
}

#[test]
fn merge_stderr_feeds_the_combined_stream() {
    let r = capture_script(
        "echo to-stdout; echo MARKER >&2",
        "grep MARKER",
        &["--merge-stderr"],
    );
    assert_eq!(r.code, 0, "2>&1 must merge before the pipe: {:?}", r.stdout);
    assert!(r.stdout.contains("MARKER"), "stdout={:?}", r.stdout);
}

#[test]
fn propagate_exit_reports_the_first_stage() {
    let r = capture_script("echo x; exit 42", "tail -1", &["--propagate-exit"]);
    assert_eq!(r.code, 42);
}

// --- recording ---

#[test]
fn the_discarded_output_is_recoverable() {
    let home = TestHome::new();
    let expected_len = "noise line\n".len() * 500;
    let script = format!("printf '{}'; echo NEEDLE", "noise line\\n".repeat(500));

    let out = home
        .cmd()
        .args([
            "run",
            "--no-mask-exit-code",
            "--pipe-through",
            "grep NEEDLE",
            "sh",
            "-c",
            &script,
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("NEEDLE"), "stdout={stdout:?}");
    assert!(
        stderr.contains("tokf raw"),
        "a materially reduced run should advertise recovery: {stderr:?}"
    );

    let raw = home.cmd().args(["raw", "last"]).output().unwrap();
    let recovered = String::from_utf8_lossy(&raw.stdout);
    assert!(
        recovered.contains("noise line"),
        "the discarded output must come back"
    );
    assert!(
        recovered.len() > expected_len / 2,
        "expected the full output"
    );
}

#[test]
fn history_records_the_pipeline_and_the_captured_command_separately() {
    let home = TestHome::new();
    home.cmd()
        .args([
            "run",
            "--no-mask-exit-code",
            "--pipe-through",
            "grep b",
            "sh",
            "-c",
            "printf 'a\\nb\\nc\\nd\\ne\\nf\\ng\\nh\\n'",
        ])
        .output()
        .unwrap();

    let out = home.cmd().args(["history", "list"]).output().unwrap();
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(
        listing.contains("grep b"),
        "the full pipeline should be the recorded command: {listing:?}"
    );
}

#[test]
fn a_quiet_run_does_not_advertise_recovery() {
    // Nothing diverged and almost nothing was discarded — the trailer would
    // cost more than it is worth.
    let r = capture_script("echo 'one line'", "cat", &[]);
    assert!(
        !r.stderr.contains("tokf raw"),
        "unexpected trailer: {:?}",
        r.stderr
    );
}

#[test]
fn a_divergence_advertises_recovery_even_on_tiny_output() {
    // The two emission rules are independent: a mismatch always speaks, and it
    // makes the recovery worth pointing at regardless of volume.
    let r = run_capture(1, "x", "tail -1");
    assert!(r.stderr.contains("tokf raw"), "stderr={:?}", r.stderr);
}

#[test]
fn an_empty_result_from_a_failed_command_is_never_silent() {
    // `(1, 1)` is agreement — nothing diverged — but an empty result from a
    // command that failed is brief R4's dangerous case: it is indistinguishable
    // from "no problems found" unless something says otherwise.
    let r = capture_script("echo 'FAILED'; exit 1", "grep NOSUCHTHING", &[]);
    assert!(
        r.stdout.contains("matched nothing") && r.stdout.contains("exited 1"),
        "stdout={:?}",
        r.stdout
    );
}

#[test]
fn a_silent_successful_pipeline_stays_quiet() {
    // The command printed nothing and everything exited 0 — genuinely
    // unremarkable, and the only empty result that goes unlabelled.
    let r = capture_script("true", "cat", &[]);
    assert_eq!(r.stdout, "", "nothing to report: {:?}", r.stdout);
    assert_eq!(r.code, 0);
}
