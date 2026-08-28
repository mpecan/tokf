//! Pipeline capture at runtime: run the first stage, pipe it onward, and
//! report any disagreement between the two exit codes.
//!
//! `cargo test | tail -10 | grep ERROR` reaches here as
//! `tokf run --pipe-through 'tail -10 | grep ERROR' cargo test`. tokf runs
//! `cargo test` itself, feeds its output through the rest of the pipeline, and
//! prints the result verbatim — so the caller gets exactly what it asked for.
//!
//! What tokf adds is that it is now the only thing that sees *both* exit codes.
//! A pipeline reports its last stage's status, so `just check 2>&1 | tail -8`
//! reports `tail`'s success and hides nine failing tests. **Difference is the
//! signal**: whenever the two codes disagree, that is said out loud.
//!
//! The process exit code is left exactly as the shell would have produced it.
//! Reporting rather than repairing is what makes capture safe to apply broadly:
//! nothing downstream changes, so `cmd | grep -q x && …` cannot invert.

use std::time::{Duration, Instant};

use tokf::pipe_exec::{self, PipeSpec, Stderr};
use tokf::runtime::Runtime;
use tokf::{history, telemetry, tracking};

use crate::Cli;
use crate::marker;
use crate::resolve;

/// Ratio at which the discarded output is worth advertising a recovery id for.
/// Below it the trailer costs more tokens than it saves the reader.
const MATERIAL_DISCARD_RATIO: usize = 4;

/// Ceiling for the captured pipeline. Far longer than a baseline's: this
/// processes a real command's full output, not a sample.
// `Duration::from_mins` is still unstable, so seconds it is.
#[allow(clippy::duration_suboptimal_units)]
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy)]
pub struct CaptureRequest<'a> {
    /// The first stage, as argv.
    pub command_args: &'a [String],
    /// Everything after the first bare pipe, run via `sh -c`.
    pub pipe_through: &'a str,
    /// True when the caller wrote `2>&1` before the pipe, so the pipeline must
    /// be fed the combined stream rather than stdout alone.
    pub merge_stderr: bool,
    /// Exit with the first stage's code instead of the pipeline's.
    pub propagate_exit: bool,
}

/// What is worth saying about a completed capture.
///
/// Emptiness is an input rather than a second decision made elsewhere: an
/// empty result from a command that failed is the same "you did not get an
/// answer" situation whether or not the codes happened to differ, and having
/// one classifier means one message vocabulary and one emit site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Report {
    /// Producer failed, pipeline reported success. The defect this exists for.
    FalseGreen,
    /// Nothing came out and the command had failed — indistinguishable from
    /// "no problems found" unless it is labelled.
    NoAnswer,
    /// Producer succeeded, consumer did not.
    ConsumerOnly,
    /// Both failed, with different codes; only the producer's is the verdict.
    BothFailed,
}

/// Classify a completed capture.
///
/// Equal codes over non-empty output are agreement — including `(1, 1)`: the
/// shell already reports the failure and there is nothing to contradict.
const fn classify(head: i32, tail: i32, stdout_empty: bool) -> Option<Report> {
    if head != 0 && tail == 0 {
        return Some(Report::FalseGreen);
    }
    // Any non-zero code with nothing to show: the caller got no answer, and an
    // empty result reading as "no problems found" is the misread. Free to say,
    // because stdout is empty anyway. `(0, 0)` with no output is the genuinely
    // unremarkable case and stays silent.
    if stdout_empty && (head != 0 || tail != 0) {
        return Some(Report::NoAnswer);
    }
    if head == tail {
        return None;
    }
    if head == 0 {
        return Some(Report::ConsumerOnly);
    }
    Some(Report::BothFailed)
}

fn report_line(r: Report, head_cmd: &str, head: i32, tail: i32) -> String {
    match r {
        Report::FalseGreen => format!(
            "Error: `{head_cmd}` exited {head} but the pipeline reported {tail} — this is NOT a pass."
        ),
        Report::NoAnswer => format!(
            "[tokf] no output — `{head_cmd}` exited {head}; the pipeline matched nothing (exit {tail})."
        ),
        Report::ConsumerOnly => {
            format!("[tokf] `{head_cmd}` exited {head}; the pipeline exited {tail}.")
        }
        Report::BothFailed => {
            format!("[tokf] `{head_cmd}` exited {head} (the verdict); the pipeline exited {tail}.")
        }
    }
}

/// Where the line belongs.
///
/// A false green goes to stdout: the whole failure mode is that nothing
/// contradicted the output. An empty stdout also takes the line, because an
/// empty result reading as "no failures found" is precisely the misread — and
/// there it costs nothing.
const fn line_goes_to_stdout(r: Report, stdout_empty: bool) -> bool {
    matches!(r, Report::FalseGreen | Report::NoAnswer) || stdout_empty
}

pub fn run_captured(
    rt: &Runtime,
    req: CaptureRequest<'_>,
    cli: &Cli,
    reporter: &dyn telemetry::TelemetryReporter,
) -> anyhow::Result<i32> {
    let start = Instant::now();
    let head_cmd = req.command_args.join(" ");
    let full_command = format!("{head_cmd} | {}", req.pipe_through);

    let (head_result, _) = resolve::run_command(
        rt,
        resolve::ResolvedCommand {
            filter_cfg: None,
            words_consumed: 0,
            matched_command: None,
            command_args: req.command_args,
            // Nothing was consumed by a filter, so everything after the
            // program name is an argument to it.
            remaining_args: &req.command_args[1..],
            verbose: cli.verbose,
        },
    )?;

    let feed = if req.merge_stderr {
        &head_result.combined
    } else {
        &head_result.stdout
    };

    let Some(piped) = run_pipeline_or_fall_back(feed, &head_result, &req) else {
        return Ok(head_result.exit_code);
    };

    // Only stdout was fed to the pipeline, so the head's stderr still has to
    // reach the caller. When `2>&1` merged it, it already went through.
    if !req.merge_stderr && !head_result.stderr.is_empty() {
        eprintln!("{}", head_result.stderr);
    }

    let out = piped.stdout.trim_end_matches('\n');
    let stdout_empty = out.is_empty();
    let report = classify(head_result.exit_code, piped.exit_code, stdout_empty);

    let raw_len = head_result.combined.len();
    let captured = Captured {
        full_command: &full_command,
        head_cmd: &head_cmd,
        head: &head_result,
        piped: &piped,
    };
    emit_report(report, &captured, stdout_empty);

    if !stdout_empty {
        println!("{out}");
    }

    // Recorded after printing: the history write can be a multi-megabyte
    // INSERT plus a retention DELETE, and nothing above needs its id.
    let history_id = record_history(rt, &captured);
    emit_recovery_trailer(rt, history_id, raw_len, out.len(), report.is_some());

    record_tracking(rt, &captured, &req, raw_len);
    resolve::try_auto_sync(rt);

    report_telemetry(rt, reporter, &captured, raw_len, start);

    Ok(if req.propagate_exit {
        head_result.exit_code
    } else {
        piped.exit_code
    })
}

/// Run the pipeline, or fail open.
///
/// When the pipeline cannot be reproduced tokf hands back the command's own
/// output rather than swallowing it — the same rule as passthrough on a
/// missing filter.
fn run_pipeline_or_fall_back(
    feed: &str,
    head: &tokf::runner::CommandResult,
    req: &CaptureRequest<'_>,
) -> Option<pipe_exec::PipeOutput> {
    // stderr is inherited: this is the pipeline the caller actually asked for,
    // so anything it complains about belongs to them.
    let spec = PipeSpec {
        timeout: CAPTURE_TIMEOUT,
        stderr: Stderr::Inherit,
    };
    let out = pipe_exec::run(feed, req.pipe_through, spec).ok();
    if out.is_none() {
        eprintln!(
            "[tokf] warning: could not run `{}` — showing unfiltered output",
            req.pipe_through
        );
        if !head.combined.is_empty() {
            println!("{}", head.combined);
        }
    }
    out
}

fn report_telemetry(
    rt: &Runtime,
    reporter: &dyn telemetry::TelemetryReporter,
    c: &Captured<'_>,
    raw_len: usize,
    start: Instant,
) {
    let shown = c.piped.stdout.len();
    reporter.report(&telemetry::TelemetryEvent::new(
        rt,
        None,
        c.full_command.to_string(),
        shown,
        shown,
        raw_len,
        &c.head.combined,
        &c.piped.stdout,
        start.elapsed(),
        // The first stage's code: the verdict, not the pipeline's report of it.
        c.head.exit_code,
    ));
}

/// Say out loud whatever the classifier found.
fn emit_report(report: Option<Report>, c: &Captured<'_>, stdout_empty: bool) {
    let Some(r) = report else {
        return;
    };
    let line = report_line(r, c.head_cmd, c.head.exit_code, c.piped.exit_code);
    if line_goes_to_stdout(r, stdout_empty) {
        println!("{line}");
    } else {
        eprintln!("{line}");
    }
}

/// Advertise `tokf raw <id>` when the recovery is worth its ~20 tokens: either
/// something diverged, or the pipeline threw away most of the output.
///
/// Kept independent of the divergence rule on purpose. Gating the mismatch line
/// on a volume heuristic would make the important signal subject to a threshold
/// tuned for the unimportant one.
fn emit_recovery_trailer(
    rt: &Runtime,
    history_id: Option<i64>,
    raw_len: usize,
    shown_len: usize,
    diverged: bool,
) {
    let Some(id) = history_id else {
        return;
    };
    if !marker::load_render_config(rt).show_indicator {
        return;
    }
    let material = raw_len > shown_len.saturating_mul(MATERIAL_DISCARD_RATIO);
    if !diverged && !material {
        return;
    }
    // The parenthetical is dropped when nothing was actually discarded — which
    // happens whenever a divergence makes the trailer worth printing on a
    // pipeline that showed everything.
    match raw_len.saturating_sub(shown_len) {
        0 => eprintln!("[tokf] full output: tokf raw {id}"),
        discarded => eprintln!("[tokf] full output: tokf raw {id} (~{discarded} bytes discarded)"),
    }
}

/// One completed capture, as the recording functions see it.
struct Captured<'a> {
    /// The pipeline as the caller wrote it.
    full_command: &'a str,
    /// The first stage alone.
    head_cmd: &'a str,
    head: &'a tokf::runner::CommandResult,
    piped: &'a pipe_exec::PipeOutput,
}

fn record_history(rt: &Runtime, c: &Captured<'_>) -> Option<i64> {
    let max_bytes = tokf::rewrite::load_user_config(rt)
        .and_then(|cfg| cfg.pipe)
        .unwrap_or_default()
        .capture_max_bytes;
    if c.head.combined.len() > max_bytes {
        eprintln!(
            "[tokf] note: {} bytes of output not recorded (over capture_max_bytes)",
            c.head.combined.len()
        );
        return None;
    }
    history::try_record(
        rt,
        &history::RecordedRun {
            command: c.full_command,
            // Reuses `executed_command` at its exact meaning: the captured
            // bytes came from this, not from the command as typed.
            executed_command: Some(c.head_cmd),
            filter_name: None,
            raw_output: &c.head.combined,
            filtered_output: &c.piped.stdout,
            // The first stage's code — the one worth having when reading
            // history back.
            exit_code: c.head.exit_code,
        },
    )
}

/// Record the run with **zero savings attributed to tokf**.
///
/// `input_bytes == output_bytes` means `input_tokens_est - output_tokens_est`
/// is 0, and savings are derived at query time — so tokf claims nothing here
/// structurally, not by convention. `raw_bytes` still carries what the caller's
/// own pipe discarded, and `head_exit_code` makes the swallowed status a query.
fn record_tracking(rt: &Runtime, c: &Captured<'_>, req: &CaptureRequest<'_>, raw_len: usize) {
    let shown = c.piped.stdout.len();
    let mut event = tracking::build_event(
        c.full_command,
        None,
        None,
        shown,
        shown,
        raw_len,
        0,
        c.piped.exit_code,
        false,
    );
    event.pipeline_tail = Some(req.pipe_through.to_string());
    event.head_exit_code = Some(c.head.exit_code);
    resolve::persist_event(rt, &mut event);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn equal_codes_over_real_output_are_agreement() {
        assert_eq!(classify(0, 0, false), None);
        // (1, 1) too: the shell already reports the failure.
        assert_eq!(classify(1, 1, false), None);
        assert_eq!(classify(101, 101, false), None);
    }

    #[test]
    fn producer_failure_hidden_by_a_successful_pipeline_is_a_false_green() {
        assert_eq!(classify(1, 0, false), Some(Report::FalseGreen));
        assert_eq!(classify(101, 0, false), Some(Report::FalseGreen));
    }

    #[test]
    fn consumer_only_failure_is_distinct() {
        assert_eq!(classify(0, 1, false), Some(Report::ConsumerOnly));
    }

    #[test]
    fn different_non_zero_codes_still_diverge() {
        assert_eq!(classify(1, 2, false), Some(Report::BothFailed));
    }

    #[test]
    fn an_empty_result_from_a_failed_command_is_never_silent() {
        // Agreement by exit code, but the caller sees nothing at all — the
        // case that reads as "no problems found".
        assert_eq!(classify(1, 1, true), Some(Report::NoAnswer));
        // Producer fine, filter matched nothing — still an unanswered question.
        assert_eq!(classify(0, 1, true), Some(Report::NoAnswer));
    }

    #[test]
    fn an_empty_result_from_a_successful_command_stays_silent() {
        assert_eq!(classify(0, 0, true), None);
    }

    #[test]
    fn a_false_green_outranks_emptiness() {
        // Both could apply; the false green is the more urgent framing.
        assert_eq!(classify(1, 0, true), Some(Report::FalseGreen));
    }

    #[test]
    fn the_urgent_reports_always_reach_stdout() {
        assert!(line_goes_to_stdout(Report::FalseGreen, false));
        assert!(line_goes_to_stdout(Report::NoAnswer, false));
    }

    #[test]
    fn an_empty_result_takes_the_line_whatever_the_report() {
        assert!(line_goes_to_stdout(Report::ConsumerOnly, true));
        assert!(!line_goes_to_stdout(Report::ConsumerOnly, false));
    }

    #[test]
    fn the_false_green_line_names_both_codes_and_says_not_a_pass() {
        let line = report_line(Report::FalseGreen, "just check", 1, 0);
        assert!(line.starts_with("Error:"), "{line}");
        assert!(line.contains("just check"), "{line}");
        assert!(line.contains("exited 1"), "{line}");
        assert!(line.contains("NOT a pass"), "{line}");
    }

    #[test]
    fn an_empty_match_is_labelled_as_a_non_answer() {
        let line = report_line(Report::NoAnswer, "cargo test", 101, 1);
        assert!(line.contains("matched nothing"), "{line}");
        assert!(line.contains("101"), "{line}");
    }
}
