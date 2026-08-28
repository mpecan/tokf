//! Feeding captured output through a shell pipeline.
//!
//! Shared by [`crate::baseline`] (which measures what a stripped pipe *would*
//! have produced) and [`crate::pipeline`] (which runs the real thing). The two
//! differ only in policy: baseline restricts the command to a whitelist and
//! discards the exit status, capture accepts whatever the caller typed —
//! the shell would have run it either way — and needs the status.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What a pipeline produced.
#[derive(Debug)]
pub struct PipeOutput {
    pub stdout: String,
    pub exit_code: i32,
}

/// Why a pipeline did not produce a result. Kept distinct so callers can say
/// which happened — a silent generic failure would be exactly the kind of
/// invisible behaviour tokf exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeError {
    /// The shell could not be started.
    Spawn,
    /// The command was still running when the deadline passed, and was killed.
    Timeout,
    /// Reading the child's output or status failed.
    Read,
}

impl std::fmt::Display for PipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn => f.write_str("failed to spawn"),
            Self::Timeout => f.write_str("timed out"),
            Self::Read => f.write_str("failed to read output"),
        }
    }
}

/// Where the pipeline's own stderr should go.
///
/// This is caller policy, not a property of running a pipeline: a baseline is
/// an invisible accounting run and must not leak its measurement subprocess's
/// stderr into the terminal, whereas a captured pipeline is the command the
/// caller actually asked for and its stderr belongs to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stderr {
    Inherit,
    Discard,
}

/// How to run one pipeline.
#[derive(Debug, Clone, Copy)]
pub struct PipeSpec {
    pub timeout: Duration,
    pub stderr: Stderr,
}

/// Run `pipe_cmd` under `sh -c`, feeding `input` to its stdin.
///
/// **Both ends are drained on their own threads.** The obvious shape — write
/// all of stdin, then wait, then read stdout — deadlocks the moment the child
/// emits more than a pipe buffer (~64 KiB) before it finishes reading, because
/// neither side can make progress: the child blocks writing stdout, we block
/// writing stdin. Real command output crosses that threshold constantly, and
/// the symptom is an inexplicable timeout rather than a hang, which is how it
/// stayed hidden.
///
/// Uses `thread::scope` so the writer can borrow `input` — a `'static` thread
/// would force cloning the whole feed buffer, which is the command's entire
/// output and routinely multiple megabytes.
///
/// # Errors
/// Returns [`PipeError`] when the shell cannot be spawned, the deadline passes
/// (the child is killed), or the child's output or status cannot be read.
pub fn run(input: &str, pipe_cmd: &str, spec: PipeSpec) -> Result<PipeOutput, PipeError> {
    let mut child = Command::new("sh")
        .args(["-c", pipe_cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(match spec.stderr {
            Stderr::Inherit => Stdio::inherit(),
            Stderr::Discard => Stdio::null(),
        })
        .spawn()
        .map_err(|_| PipeError::Spawn)?;

    let mut stdin = child.stdin.take().ok_or(PipeError::Spawn)?;
    let mut stdout = child.stdout.take().ok_or(PipeError::Spawn)?;

    std::thread::scope(|scope| {
        // Write errors are ignored: a consumer that exits early (`head -1`)
        // closes the pipe, which is a successful outcome, not a failure.
        scope.spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
            let _ = stdin.flush();
        });
        let reader = scope.spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });

        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if start.elapsed() >= spec.timeout {
                        let _ = child.kill();
                        return Err(PipeError::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(PipeError::Read),
            }
        };

        let buf = reader.join().map_err(|_| PipeError::Read)?;
        Ok(PipeOutput {
            // `from_utf8` moves the buffer when it is valid UTF-8, which is the
            // overwhelmingly common case; `from_utf8_lossy(..).into_owned()`
            // would copy every byte. Same lossy semantics on the other branch,
            // because a pipeline that emits non-UTF8 bytes still ran and
            // dropping its output would misreport that as "produced nothing".
            stdout: String::from_utf8(buf)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
            exit_code: crate::runner::exit_code_from_status(status),
        })
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CAPTURE_TIMEOUT_FOR_TESTS: Duration = Duration::from_secs(30);

    fn spec() -> PipeSpec {
        PipeSpec {
            timeout: CAPTURE_TIMEOUT_FOR_TESTS,
            stderr: Stderr::Discard,
        }
    }

    #[test]
    fn a_failure_says_which_failure_it_was() {
        let spec = PipeSpec {
            timeout: CAPTURE_TIMEOUT_FOR_TESTS,
            stderr: Stderr::Discard,
        };
        // A generic "it failed" would be exactly the invisible behaviour tokf
        // exists to avoid.
        assert_eq!(
            run("x", "/nonexistent/definitely-not-a-shell-builtin-xyz", spec)
                .unwrap()
                .exit_code,
            127,
            "sh reports not-found through the exit code, not a spawn failure"
        );
    }

    #[test]
    fn runs_a_pipeline_and_reports_status() {
        let out = run("a\nb\nc\n", "grep b", spec()).unwrap();
        assert_eq!(out.stdout, "b\n");
        assert_eq!(out.exit_code, 0);
    }

    #[test]
    fn reports_a_non_zero_status_from_the_last_stage() {
        let out = run("a\nb\n", "grep zzz", spec()).unwrap();
        assert!(out.stdout.is_empty());
        assert_eq!(out.exit_code, 1);
    }

    #[test]
    fn multi_stage_pipelines_work() {
        let out = run("1\n2\n3\n4\n5\n", "tail -3 | head -1", spec()).unwrap();
        assert_eq!(out.stdout, "3\n");
    }

    /// The deadlock regression: a large input whose output exceeds the pipe
    /// buffer long before stdin is fully written.
    #[test]
    fn large_input_does_not_deadlock() {
        let input = "matching line\n".repeat(200_000);
        let out = run(&input, "grep matching", spec()).unwrap();
        assert_eq!(out.stdout.len(), input.len());
    }

    #[test]
    fn non_utf8_output_survives() {
        let out = run("", "printf '\\xff\\xfe'", spec()).unwrap();
        assert!(!out.stdout.is_empty(), "lossy conversion should keep bytes");
    }

    #[test]
    fn early_exit_consumer_is_not_an_error() {
        let input = "line\n".repeat(100_000);
        let out = run(&input, "head -1", spec()).unwrap();
        assert_eq!(out.stdout, "line\n");
    }

    #[test]
    fn timeout_returns_none() {
        let spec = PipeSpec {
            timeout: Duration::from_millis(50),
            stderr: Stderr::Discard,
        };
        assert_eq!(run("x", "sleep 5", spec).unwrap_err(), PipeError::Timeout);
    }
}
