//! Pipeline capture: deciding when tokf can run a pipeline itself.
//!
//! When enabled (`[pipe] capture = true`), pipelines that the existing
//! pipe-stripping path declines are rewritten so tokf runs the *first* stage,
//! feeds its output through the rest of the pipeline, and can therefore observe
//! both exit codes. See `crate::pipeline` for the runtime half.
//!
//! Everything here is a *decline* decision. tokf fails open: when any check
//! below is unsure, the command passes through untouched and behaves exactly as
//! it does today. The rules are deliberately about whether tokf can physically
//! reproduce the pipeline — not about what the caller intends to do with the
//! exit code, because in the default `capture_exit = "report"` mode the exit
//! code is left alone and no downstream intent can be broken.

use tokf_hook_types::{CaptureExit, PipeConfig};

use super::bash_ast::{ParsedCommand, StrippedPipe};
use super::rules::apply_rules;
use super::types::RewriteOptions;
use super::{SegmentRules, inject_pipe_flags_with_options, segment_filter_match};

/// A pipeline tokf has decided it can capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// The first stage, run by tokf directly (e.g. `cargo test`).
    pub head: String,
    /// Everything after the first bare pipe, run via `sh -c` (e.g. `tail -10 | grep ERROR`).
    pub tail: String,
    /// True when the head merges stderr into stdout (`2>&1`), meaning the
    /// pipeline must be fed the combined stream rather than stdout alone.
    pub merge_stderr: bool,
}

/// Consumers that cannot be fed a captured buffer: pagers and interactive UIs.
/// Capture-then-feed either hangs them or produces nothing useful.
/// `watch`/`top`/`htop` are deliberately absent: they never terminate at all,
/// so [`NEVER_TERMINATES`] already covers them — and covers them on the *head*
/// side too, which this list does not.
const INTERACTIVE_CONSUMERS: &[&str] = &[
    "less", "more", "most", "fzf", "vim", "vi", "nvim", "emacs", "nano", "man",
];

/// Commands that never terminate at all, whatever their arguments.
const NEVER_TERMINATES: &[&str] = &["yes", "watch", "top", "htop"];

/// Commands that terminate normally but follow their input forever under
/// `-f`/`--follow`. Under capture these are far worse than today: a live
/// pipeline still shows partial output when the caller's timeout fires,
/// whereas capture buffers everything and shows nothing.
const FOLLOWABLE: &[&str] = &["tail", "journalctl"];

/// Container CLIs where only the `logs` subcommand follows. `docker ps -f
/// status=running` uses `-f` as a *filter*, so the subcommand must be checked
/// before the flag or every `ps` invocation would be declined.
const FOLLOWABLE_SUBCOMMAND: &[(&str, &str)] =
    &[("docker", "logs"), ("podman", "logs"), ("kubectl", "logs")];

/// Plan a capture for `parsed`, or return `None` to leave the command alone.
///
/// `parsed` must be a single compound segment that
/// [`ParsedCommand::has_bare_pipe`] reports true for.
pub fn plan(parsed: &ParsedCommand, cfg: &PipeConfig) -> Option<Capture> {
    if !cfg.capture {
        return None;
    }
    let source = parsed.source();
    let positions = parsed.pipe_positions();
    let first = *positions.first()?;

    let raw_head = source.get(..first)?.trim_end();
    let tail = source.get(first + 1..)?.trim().to_string();
    if raw_head.is_empty() || tail.is_empty() {
        return None;
    }

    let merge_stderr = head_merges_stderr(parsed)?;
    // The `2>&1` has to come *off* the head. Once the command is rewritten the
    // shell applies that redirect to the `tokf run` process itself, not to the
    // captured command — so it would merge tokf's own notes into stdout while
    // still feeding the pipeline stdout alone, which is the opposite of what
    // the caller wrote. `--merge-stderr` carries the intent instead.
    let head = if merge_stderr {
        strip_trailing_stderr_merge(raw_head)?
    } else {
        raw_head
    };
    if head.is_empty() || head_is_unbounded(head) {
        return None;
    }

    // Stage names come from the AST, never from splitting on `'|'`: a pipe
    // inside quotes (`grep -E 'a|b'`) would otherwise shred the stage names and
    // silently stop the denylist — and `propagate_is_unsafe` — from matching.
    for stage in parsed.pipeline_stages().iter().skip(1) {
        if stage_is_denied(stage, cfg) {
            return None;
        }
    }

    Some(Capture {
        head: head.to_string(),
        tail,
        merge_stderr,
    })
}

/// Additional declines that only apply when capture mode also *changes* the
/// exit code — there, callers that consume the pipeline's status on purpose
/// would silently invert.
pub fn propagate_is_unsafe(cfg: &PipeConfig, parsed: &ParsedCommand, followed_by: &str) -> bool {
    if cfg.capture_exit != CaptureExit::Propagate {
        return false;
    }
    let sep = followed_by.trim();
    if sep.starts_with("&&") || sep.starts_with("||") {
        return true;
    }
    // The AST's last stage, not `tail.rsplit('|')`: for `grep -q 'a|b'` the
    // naive split yields `b'`, so this guard silently failed to fire on
    // exactly the predicate pipelines it exists to protect.
    let stages = parsed.pipeline_stages();
    let Some(last) = stages.last() else {
        return false;
    };
    let mut words = last.split_whitespace();
    match words.next() {
        Some("test" | "[") => true,
        Some("grep") => words.any(|w| w == "-q" || w == "--quiet" || is_short_flag_with(w, b'q')),
        _ => false,
    }
}

/// Remove a **trailing** `2>&1` from a head whose redirects have already been
/// validated as nothing but stderr merges.
///
/// Byte-preserving by construction: it slices the token off the end rather than
/// re-tokenising. Re-splitting on whitespace and re-joining would rewrite the
/// rest of the command — `git commit -m "a  b" 2>&1 | tail` would come back
/// with the double space collapsed and the quotes redistributed.
///
/// Returns `None` for the exotic non-trailing forms (`cmd 2>&1 arg`), which are
/// declined rather than guessed at.
fn strip_trailing_stderr_merge(head: &str) -> Option<&str> {
    let rest = head.trim_end().strip_suffix("2>&1")?.trim_end();
    (!rest.is_empty()).then_some(rest)
}

/// Does the head merge stderr into stdout?
///
/// Returns `Some(true)` when the head merges stderr into stdout (`2>&1`),
/// `Some(false)` when it has no redirects at all, and `None` — decline — for
/// anything else. `cmd 2>/dev/null | grep x` is the case that matters: tokf
/// forwards the head's stderr to its own, which would resurrect output the
/// caller explicitly silenced.
fn head_merges_stderr(parsed: &ParsedCommand) -> Option<bool> {
    let redirects = parsed.first_command_redirects();
    if redirects.is_empty() {
        return Some(false);
    }
    let all_merge_stderr = redirects
        .iter()
        .all(|(fd, op, target)| *fd == 2 && op.contains(">&") && target == "1");
    if all_merge_stderr { Some(true) } else { None }
}

fn head_is_unbounded(head: &str) -> bool {
    let mut words = head.split_whitespace().peekable();
    // Step over wrappers that do not change what the command is.
    while let Some(w) = words.peek() {
        if w.contains('=') && !w.starts_with('-') {
            words.next();
            continue;
        }
        match *w {
            "timeout" | "env" | "time" | "nice" | "sudo" | "command" | "exec" | "stdbuf" => {
                words.next();
            }
            _ => break,
        }
    }
    let Some(name) = words.next().map(crate::config::extract_basename) else {
        return false;
    };
    let rest: Vec<&str> = words.collect();
    is_follow_invocation(name, &rest)
}

fn stage_is_denied(stage: &str, cfg: &PipeConfig) -> bool {
    let mut words = stage.split_whitespace();
    let Some(name) = words.next().map(crate::config::extract_basename) else {
        return true;
    };
    let rest: Vec<&str> = words.collect();
    if cfg.capture_deny.iter().any(|d| d == name) {
        return true;
    }
    INTERACTIVE_CONSUMERS.contains(&name) || is_follow_invocation(name, &rest)
}

/// True when this invocation follows its input indefinitely (`-f`/`--follow`),
/// or is a command that never terminates at all.
fn is_follow_invocation(name: &str, args: &[&str]) -> bool {
    if NEVER_TERMINATES.contains(&name) {
        return true;
    }
    let followable = FOLLOWABLE.contains(&name)
        || FOLLOWABLE_SUBCOMMAND
            .iter()
            .any(|(cmd, sub)| *cmd == name && args.first().is_some_and(|a| a == sub));
    followable && args.iter().any(|a| has_follow_flag(a))
}

fn has_follow_flag(arg: &str) -> bool {
    arg == "--follow" || is_short_flag_with(arg, b'f')
}

/// True for a clustered short flag containing `byte`, e.g. `-qi` for `b'q'`.
/// Long flags (`--foo`) are never short-flag clusters.
///
/// `pub` because `crate::shell` asks the same question about `-c`.
pub fn is_short_flag_with(arg: &str, byte: u8) -> bool {
    arg.starts_with('-')
        && !arg.starts_with("--")
        && arg.len() > 1
        && arg.as_bytes()[1..].contains(&byte)
}

/// Everything [`rewrite_piped_segment`] needs about the segment it decides on.
#[derive(Clone, Copy)]
pub(super) struct PipedSegment<'a> {
    /// The segment exactly as written, returned unchanged when nothing applies.
    pub(super) segment: &'a str,
    /// The separator that follows this segment in a compound command.
    pub(super) sep: &'a str,
    /// The segment with any leading `KEY=VALUE` assignments removed.
    pub(super) cmd: &'a str,
    /// Those assignments, re-prepended to whatever is produced.
    pub(super) env_prefix: &'a str,
    pub(super) parsed: Option<&'a ParsedCommand>,
}

/// Decide what to do with a segment that contains a bare pipe.
///
/// Three outcomes in priority order: strip the pipe into `--baseline-pipe`
/// (today's behaviour, needs a matching filter), capture the pipeline, or leave
/// it alone — with the wrapper rules still getting their turn before that last
/// fallback.
pub(super) fn rewrite_piped_segment(
    seg: PipedSegment<'_>,
    rules: &SegmentRules<'_>,
    verbose: bool,
) -> String {
    let env_prefix = seg.env_prefix;

    if rules.pipe.strip
        && let Some(StrippedPipe { base, suffix }) =
            seg.parsed.and_then(ParsedCommand::strip_simple_pipe)
        && let Some(rewritten) = segment_filter_match(&base, rules)
    {
        if verbose {
            eprintln!("[tokf] stripped pipe — tokf filter provides structured output");
        }
        let injected = inject_pipe_flags_with_options(
            &rewritten,
            &suffix,
            rules.pipe.prefer_less,
            rules.options,
        );
        return format!("{env_prefix}{injected}");
    }

    // Capture mode. This deliberately runs *before* the wrapper result is used:
    // the `just`/`make` wrapper patterns end in `(\s.*)?$`, which swallows
    // ` check | tail -5` whole, so letting the wrapper claim the segment here
    // would return the pipeline untouched — and the swallowed exit code with
    // it. Wrapper rules still apply to the captured *head*, so recipe-line
    // filtering is kept, nested inside the capture.
    if !rules.pipefail_handled
        && let Some(parsed) = seg.parsed
        && let Some(cap) = plan(parsed, rules.pipe)
        && !propagate_is_unsafe(rules.pipe, parsed, seg.sep)
    {
        if verbose {
            eprintln!(
                "[tokf] pipeline capture: running `{}` under tokf, then `{}`",
                cap.head, cap.tail
            );
        }
        // Wrapper rules still apply — to the captured head — so `just`'s
        // recipe-line filtering is kept, nested inside the capture.
        let head = apply_rules(rules.wrapper, &cap.head).unwrap_or_else(|| cap.head.clone());
        let injected = build_capture_command(&head, &cap, rules.pipe, rules.options);
        return format!("{env_prefix}{injected}");
    }

    // Capture declined — fall back to today's behaviour, where a wrapper rule
    // may still claim the whole segment (`make check | tee log.txt` becomes
    // `make SHELL=tokf check | tee log.txt`).
    if let Some(wrapped) = apply_rules(rules.wrapper, seg.cmd) {
        return super::wrapper_rewrite(env_prefix, &wrapped, verbose);
    }
    if verbose {
        eprintln!("[tokf] skipping rewrite: command contains a pipe");
    }
    seg.segment.to_string()
}

/// Build `tokf run --pipe-through '<tail>' <head>` for pipeline capture.
///
/// The tail is passed as a single argument, escaped with the same `'\''` idiom
/// as `--baseline-pipe`, so it reaches `tokf run` byte-identical to what the
/// caller typed and is handed to `sh -c` unchanged.
fn build_capture_command(
    head: &str,
    cap: &Capture,
    pipe: &PipeConfig,
    options: &RewriteOptions,
) -> String {
    let escaped = super::single_quote(&cap.tail);
    let mask_flag = if options.no_mask_exit_code {
        " --no-mask-exit-code"
    } else {
        ""
    };
    let merge_flag = if cap.merge_stderr {
        " --merge-stderr"
    } else {
        ""
    };
    let propagate_flag = if pipe.capture_exit == CaptureExit::Propagate {
        " --propagate-exit"
    } else {
        ""
    };
    format!("tokf run{mask_flag} --pipe-through '{escaped}'{merge_flag}{propagate_flag} {head}")
}
