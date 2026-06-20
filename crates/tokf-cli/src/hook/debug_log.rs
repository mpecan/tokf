//! Append-only diagnostic log for hook invocations. Activated by setting
//! the `TOKF_HOOK_LOG` env var to a writable file path. Each invocation
//! writes one YAML record covering the BEFORE / AFTER command strings and
//! the outcome. Best-effort: any I/O error is silently dropped so a
//! missing/unwritable log path never blocks the hook (#355).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tokf_hook_types::HookFormat;

use super::HookOutcome;

const ENV_VAR: &str = "TOKF_HOOK_LOG";

/// Write a single hook-event record to the path in `TOKF_HOOK_LOG`, if set.
///
/// `after` is the rewritten command string when the rewrite changed the
/// input; pass `None` when the hook passed the command through unchanged.
pub(super) fn log_event(
    tool: &str,
    format: HookFormat,
    before: &str,
    after: Option<&str>,
    outcome: HookOutcome,
) {
    let Some(path) = log_path() else {
        return;
    };
    let record = format_record(&Record {
        tool,
        format,
        before,
        after,
        outcome,
        now: SystemTime::now(),
    });
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(record.as_bytes()));
}

fn log_path() -> Option<PathBuf> {
    let raw = std::env::var_os(ENV_VAR)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

struct Record<'a> {
    tool: &'a str,
    format: HookFormat,
    before: &'a str,
    after: Option<&'a str>,
    outcome: HookOutcome,
    now: SystemTime,
}

fn format_record(rec: &Record<'_>) -> String {
    let ts = rec
        .now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let outcome_str = match rec.outcome {
        HookOutcome::Allow => "Allow",
        HookOutcome::Ask => "Ask",
        HookOutcome::Deny => "Deny",
        HookOutcome::PassThrough => "PassThrough",
    };
    let after_block = rec.after.map_or_else(
        || "after: ~\n".to_string(),
        |s| format!("after: |-\n{}\n", indent(s, "  ")),
    );
    format!(
        "---\nts: {ts}\ntool: {tool}\nformat: {format}\noutcome: {outcome}\nbefore: |-\n{before}\n{after_block}",
        tool = rec.tool,
        format = rec.format.as_str(),
        outcome = outcome_str,
        before = indent(rec.before, "  "),
    )
}

fn indent(s: &str, prefix: &str) -> String {
    if s.is_empty() {
        return prefix.to_string();
    }
    s.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at_epoch(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn record_with_rewrite() {
        let r = format_record(&Record {
            tool: "Bash",
            format: HookFormat::ClaudeCode,
            before: "git status",
            after: Some("tokf run git status"),
            outcome: HookOutcome::Allow,
            now: at_epoch(1_700_000_000),
        });
        assert!(r.starts_with("---\nts: 1700000000\n"));
        assert!(r.contains("tool: Bash\n"));
        assert!(r.contains("format: claude-code\n"));
        assert!(r.contains("outcome: Allow\n"));
        assert!(r.contains("before: |-\n  git status\n"));
        assert!(r.contains("after: |-\n  tokf run git status\n"));
    }

    #[test]
    fn record_passthrough_has_null_after() {
        let r = format_record(&Record {
            tool: "Bash",
            format: HookFormat::ClaudeCode,
            before: "ls",
            after: None,
            outcome: HookOutcome::PassThrough,
            now: at_epoch(1_700_000_000),
        });
        assert!(r.contains("outcome: PassThrough\n"));
        assert!(r.contains("after: ~\n"));
    }

    #[test]
    fn multiline_before_indented_under_block_scalar() {
        // The most useful diagnostic case (#355): newline-bearing BEFORE
        // blocks must be indented so the YAML block scalar parses cleanly.
        let r = format_record(&Record {
            tool: "Bash",
            format: HookFormat::ClaudeCode,
            before: "cargo test\nls | head -1\necho hi",
            after: Some("tokf run cargo test\ntokf run --baseline-pipe 'head -1' ls\necho hi"),
            outcome: HookOutcome::Allow,
            now: at_epoch(0),
        });
        assert!(
            r.contains("before: |-\n  cargo test\n  ls | head -1\n  echo hi\n"),
            "BEFORE block not indented as expected: {r}"
        );
        assert!(
            r.contains(
                "after: |-\n  tokf run cargo test\n  tokf run --baseline-pipe 'head -1' ls\n  echo hi\n"
            ),
            "AFTER block not indented as expected: {r}"
        );
    }

    #[test]
    fn empty_input_does_not_panic() {
        let r = format_record(&Record {
            tool: "Bash",
            format: HookFormat::ClaudeCode,
            before: "",
            after: None,
            outcome: HookOutcome::PassThrough,
            now: at_epoch(0),
        });
        // Block scalar with empty body indents to a single prefix-only line.
        assert!(r.contains("before: |-\n  \n"));
    }

    #[test]
    fn gemini_and_cursor_format_strings() {
        let r_g = format_record(&Record {
            tool: "run_shell_command",
            format: HookFormat::Gemini,
            before: "ls",
            after: None,
            outcome: HookOutcome::PassThrough,
            now: at_epoch(0),
        });
        assert!(r_g.contains("format: gemini\n"));
        let r_c = format_record(&Record {
            tool: "Shell",
            format: HookFormat::Cursor,
            before: "ls",
            after: None,
            outcome: HookOutcome::PassThrough,
            now: at_epoch(0),
        });
        assert!(r_c.contains("format: cursor\n"));
    }
}
