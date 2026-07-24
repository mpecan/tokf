//! Property-based tests for the shell rewrite engine (#358).
//!
//! Example-based tests missed #355 — a multi-line compound command like
//! `head -1\necho` collapsing into a glued token (`head -1echo`) that
//! fabricated argv and wrote stray files in the agent's cwd — because the bug
//! only fired on a specific shape (multi-segment + filter match), not directly
//! via `tokf rewrite`. These properties generate bash from a deliberately
//! narrow grammar and assert structural invariants on the rewrite output,
//! hardening the engine against the broader "output looks plausible but has a
//! different argv structure" class.
//!
//! Scope is Tier 1 — static invariants only (no execution, no sandboxing).
//! Heredocs, command substitutions, and redirects are deliberately out of the
//! grammar; they are handled by their own targeted skip rules and tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use proptest::prelude::*;
use rable::ast::{Node, NodeKind};
use tempfile::TempDir;

use super::bash_ast::ParsedCommand;
use super::*;
use crate::config;

// --- Fixtures -------------------------------------------------------------

/// A single set of filter TOMLs, written once and reused by every case, so the
/// rewrite path actually fires for the biased command heads instead of every
/// input passing through unchanged. Kept in a `static` so the backing `TempDir`
/// lives for the whole test process.
static FILTER_DIR: LazyLock<(TempDir, Vec<PathBuf>)> = LazyLock::new(|| {
    let dir = TempDir::new().unwrap();
    let filters = [
        ("git-status.toml", "command = \"git status\""),
        ("cargo-test.toml", "command = \"cargo test\""),
        ("ls.toml", "command = \"ls\""),
        ("docker-ps.toml", "command = \"docker ps\""),
    ];
    for (name, body) in filters {
        fs::write(dir.path().join(name), body).unwrap();
    }
    let dirs = vec![dir.path().to_path_buf()];
    (dir, dirs)
});

fn filter_dirs() -> &'static [PathBuf] {
    &FILTER_DIR.1
}

/// Rewrite `cmd` against the shared filter set on a freshly isolated runtime,
/// with the on-disk discovery cache **bypassed**.
///
/// `cache_path` derives the manifest location from `search_dirs[0].parent()`,
/// which for our shared `TempDir` is the global system temp root — so the
/// cached path would have all five property threads (libtest runs them in
/// parallel) reading and writing one `manifest.bin` concurrently, yielding torn
/// reads and intermittent failures. The cache is a perf optimisation irrelevant
/// to what these properties assert, so `no_cache: true` keeps every case fully
/// isolated and deterministic.
fn rw(cmd: &str) -> String {
    let rt = Runtime::isolated();
    let user_config = RewriteConfig::default();
    rewrite_with_config_and_options(
        RewriteCtx {
            rt: &rt,
            user_config: &user_config,
            search_dirs: filter_dirs(),
            no_cache: true,
        },
        cmd,
        false,
        &RewriteOptions::default(),
    )
}

fn parses(src: &str) -> bool {
    rable::parse(src, false).is_ok()
}

// --- AST helpers ----------------------------------------------------------

/// Every simple command's argv (raw word values, quotes preserved) reachable
/// through lists and pipelines. Returns an empty vec when `src` doesn't parse.
fn collect_command_argvs(src: &str) -> Vec<Vec<String>> {
    let Ok(nodes) = rable::parse(src, false) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in &nodes {
        walk_commands(node, &mut out);
    }
    out
}

fn walk_commands(node: &Node, out: &mut Vec<Vec<String>>) {
    match &node.kind {
        NodeKind::Command { words, .. } => {
            let argv: Vec<String> = words.iter().filter_map(word_value).collect();
            if !argv.is_empty() {
                out.push(argv);
            }
        }
        NodeKind::Pipeline { commands, .. } => {
            for cmd in commands {
                walk_commands(cmd, out);
            }
        }
        NodeKind::List { items } => {
            for item in items {
                walk_commands(&item.command, out);
            }
        }
        _ => {}
    }
}

fn word_value(node: &Node) -> Option<String> {
    if let NodeKind::Word { value, .. } = &node.kind {
        Some(value.clone())
    } else {
        None
    }
}

/// True when `needle` appears as a contiguous run of elements within `haystack`.
fn is_contiguous_subslice(needle: &[String], haystack: &[String]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

/// Extract each single- or double-quoted literal (including its quotes). The
/// grammar never nests or escapes quotes, so the matching close quote is simply
/// the next occurrence of the same quote character.
fn quoted_spans(src: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut spans = Vec::new();
    let mut idx = 0;
    while idx < chars.len() {
        let (start, q) = chars[idx];
        if (q == '\'' || q == '"')
            && let Some(rel) = chars[idx + 1..].iter().position(|&(_, c)| c == q)
        {
            let close = idx + 1 + rel;
            let end_byte = chars[close].0;
            spans.push(src[start..=end_byte].to_string());
            idx = close + 1;
            continue;
        }
        idx += 1;
    }
    spans
}

// --- Grammar --------------------------------------------------------------

/// Reserved words that are only valid bash in a larger construct; as a bare
/// command head they make the whole input unparseable, so keep them out of the
/// random-head strategy to avoid generating throwaway inputs.
fn is_shell_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "for"
            | "while"
            | "until"
            | "do"
            | "done"
            | "case"
            | "esac"
            | "in"
            | "function"
            | "select"
            | "time"
            | "coproc"
    )
}

fn bare_word() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9_-]{0,6}")
        .unwrap()
        .prop_filter("no shell keyword heads", |w| !is_shell_keyword(w))
}

fn quoted_arg() -> impl Strategy<Value = String> {
    // Inner content excludes quotes, `$`, backslash, and newline so scanning is
    // unambiguous and nothing expands at (hypothetical) shell time.
    const INNER: &str = "[a-zA-Z0-9 ._/=:-]{0,10}";
    prop_oneof![
        proptest::string::string_regex(INNER)
            .unwrap()
            .prop_map(|s| format!("'{s}'")),
        proptest::string::string_regex(INNER)
            .unwrap()
            .prop_map(|s| format!("\"{s}\"")),
    ]
}

fn arg() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => bare_word(),
        1 => Just("-1".to_string()),
        1 => Just("-n 5".to_string()),
        1 => Just("--lines=10".to_string()),
        2 => quoted_arg(),
    ]
}

/// A command head, biased toward names that match a filter so the rewrite path
/// fires, mixed with random words that pass through unchanged.
fn command_head() -> impl Strategy<Value = String> {
    prop_oneof![
        1 => Just("git status".to_string()),
        1 => Just("cargo test".to_string()),
        1 => Just("ls".to_string()),
        1 => Just("docker ps".to_string()),
        1 => bare_word(),
    ]
}

fn simple_command() -> impl Strategy<Value = String> {
    (command_head(), prop::collection::vec(arg(), 0..4)).prop_map(|(head, args)| {
        if args.is_empty() {
            head
        } else {
            format!("{head} {}", args.join(" "))
        }
    })
}

/// A simple pipe target — the strippable suffixes the engine special-cases.
fn pipe_target() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("head".to_string()),
        Just("head -1".to_string()),
        Just("tail -5".to_string()),
        bare_word().prop_map(|w| format!("grep {w}")),
    ]
}

fn pipe_command() -> impl Strategy<Value = String> {
    (simple_command(), pipe_target()).prop_map(|(base, target)| format!("{base} | {target}"))
}

fn segment() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => simple_command(),
        1 => pipe_command(),
    ]
}

fn separator() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(" && ".to_string()),
        Just(" || ".to_string()),
        Just("; ".to_string()),
        Just("\n".to_string()),
    ]
}

/// 1–4 segments joined by chain operators — the shape that exercised #355.
fn compound() -> impl Strategy<Value = String> {
    prop::collection::vec(segment(), 1..5)
        .prop_flat_map(|segs| {
            let sep_count = segs.len() - 1;
            (
                Just(segs),
                prop::collection::vec(separator(), sep_count..=sep_count),
            )
        })
        .prop_map(|(segs, seps)| {
            let mut out = String::new();
            for (i, seg) in segs.iter().enumerate() {
                out.push_str(seg);
                if let Some(sep) = seps.get(i) {
                    out.push_str(sep);
                }
            }
            out
        })
}

// --- Properties -----------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    /// Invariant 1: `compound_segments` round-trips byte-for-byte. Reassembling
    /// `seg + sep` must reproduce the input; the exact property #355 broke.
    #[test]
    fn prop_compound_segments_roundtrip(x in compound()) {
        let Some(parsed) = ParsedCommand::parse(&x) else {
            return Ok(());
        };
        let rebuilt: String = parsed
            .compound_segments()
            .into_iter()
            .map(|(seg, sep)| seg + &sep)
            .collect();
        prop_assert_eq!(rebuilt, x);
    }

    /// Invariant 2: argv preservation. Every "real" (non-`tokf`) command in the
    /// rewrite output must appear verbatim in the input — its argv is a
    /// contiguous run of words within some input command. The engine only ever
    /// wraps a matched command in `tokf run …` or lifts a pipe suffix into a
    /// `tokf` argument; it never rewrites a passthrough command's words. So a
    /// non-`tokf` output command that is not a verbatim slice of an input
    /// command means a word was fabricated, dropped, or reordered. This catches
    /// the `head -1` → `head -1echo` gluing from #355, and (unlike a bare
    /// set-membership check) also catches drops, reorders, and coincidental
    /// same-word collisions.
    #[test]
    fn prop_argv_preserved(x in compound()) {
        if !parses(&x) {
            return Ok(());
        }
        let input_argvs = collect_command_argvs(&x);
        let output = rw(&x);
        for argv in collect_command_argvs(&output) {
            if config::extract_basename(&argv[0]) == "tokf" {
                continue;
            }
            prop_assert!(
                input_argvs
                    .iter()
                    .any(|input| is_contiguous_subslice(&argv, input)),
                "non-tokf command argv not found verbatim in input: {:?}\n  input:  {:?}\n  output: {:?}",
                argv,
                x,
                output
            );
        }
    }

    /// Invariant 3: the rewrite output is always valid shell. An unparseable
    /// rewrite is always wrong.
    #[test]
    fn prop_output_parseable(x in compound()) {
        if !parses(&x) {
            return Ok(());
        }
        let output = rw(&x);
        prop_assert!(
            parses(&output),
            "rewrite produced unparseable shell\n  input:  {:?}\n  output: {:?}",
            x,
            output
        );
    }

    /// Invariant 4: idempotence. The built-in `^tokf ` skip means a second
    /// rewrite is a no-op; a regression means double-wrapping leaked through.
    #[test]
    fn prop_idempotent(x in compound()) {
        if !parses(&x) {
            return Ok(());
        }
        let once = rw(&x);
        let twice = rw(&once);
        prop_assert_eq!(twice, once);
    }

    /// Invariant 5: quote integrity. Every quoted literal in the input survives
    /// byte-for-byte in the output — the engine never splices into an opaque
    /// quoted payload (cf. #338).
    #[test]
    fn prop_quote_integrity(x in compound()) {
        if !parses(&x) {
            return Ok(());
        }
        let output = rw(&x);
        for span in quoted_spans(&x) {
            prop_assert!(
                output.contains(&span),
                "quoted literal {:?} lost\n  input:  {:?}\n  output: {:?}",
                span,
                x,
                output
            );
        }
    }
}
