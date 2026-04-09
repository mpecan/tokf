use super::bash_ast::*;

// --- compound_segments ---

#[test]
fn single_command() {
    let p = ParsedCommand::parse("git status").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs, vec![("git status".to_string(), String::new())]);
}

#[test]
fn and_then_chain() {
    let p = ParsedCommand::parse("git add . && git status").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].0, "git add .");
    assert!(segs[0].1.contains("&&"));
    assert_eq!(segs[1].0, "git status");
    assert!(segs[1].1.is_empty());
}

#[test]
fn or_chain() {
    let p = ParsedCommand::parse("make test || cargo test").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].0, "make test");
    assert!(segs[0].1.contains("||"));
    assert_eq!(segs[1].0, "cargo test");
}

#[test]
fn semicolon_chain() {
    let p = ParsedCommand::parse("git add . ; git status").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].0, "git add .");
    assert!(segs[0].1.contains(';'));
    assert_eq!(segs[1].0.trim(), "git status");
}

#[test]
fn pipe_not_a_separator() {
    let p = ParsedCommand::parse("git diff HEAD | head -5").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].0, "git diff HEAD | head -5");
}

#[test]
fn three_segment_and_chain() {
    // Regression: rable parses left-associatively as ((A && B) && C),
    // so without recursive flattening this returned 2 segments and
    // glued the middle command into the first. Verify we now get 3.
    let p = ParsedCommand::parse("git add a && git commit -m x && git push").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].0, "git add a");
    assert!(segs[0].1.contains("&&"));
    assert_eq!(segs[1].0, "git commit -m x");
    assert!(segs[1].1.contains("&&"));
    assert_eq!(segs[2].0, "git push");
    assert!(segs[2].1.is_empty());
}

#[test]
fn mixed_and_or_chain() {
    // Locks in operator inheritance for the recursive flattener:
    // parsed as ((a && b) || c), the last child of the inner list (b)
    // must inherit the outer "||" — not its own None operator.
    let p = ParsedCommand::parse("a && b || c").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].0, "a");
    assert!(segs[0].1.contains("&&"));
    assert_eq!(segs[1].0, "b");
    assert!(segs[1].1.contains("||"));
    assert_eq!(segs[2].0, "c");
    assert!(segs[2].1.is_empty());
}

// --- pipe detection ---

#[test]
fn has_bare_pipe_basic() {
    let p = ParsedCommand::parse("git log | head -5").unwrap();
    assert!(p.has_bare_pipe());
}

#[test]
fn no_bare_pipe_plain_command() {
    let p = ParsedCommand::parse("cargo build --release").unwrap();
    assert!(!p.has_bare_pipe());
}

#[test]
fn no_bare_pipe_logical_or() {
    let p = ParsedCommand::parse("make test || cargo test").unwrap();
    assert!(!p.has_bare_pipe());
}

#[test]
fn pipe_in_quotes_not_counted() {
    let p = ParsedCommand::parse("grep -E 'foo|bar' file.txt").unwrap();
    assert!(!p.has_bare_pipe());
}

#[test]
fn pipe_in_double_quotes_not_counted() {
    let p = ParsedCommand::parse(r#"echo "a | b""#).unwrap();
    assert!(!p.has_bare_pipe());
}

#[test]
fn pipe_after_closing_quote_is_bare() {
    let p = ParsedCommand::parse(r#"echo "hello" | grep o"#).unwrap();
    assert!(p.has_bare_pipe());
}

// --- strip_simple_pipe ---

#[test]
fn strip_tail_n() {
    let p = ParsedCommand::parse("cargo test | tail -n 5").unwrap();
    assert_eq!(
        p.strip_simple_pipe(),
        Some(StrippedPipe {
            base: "cargo test".to_string(),
            suffix: "tail -n 5".to_string(),
        })
    );
}

#[test]
fn strip_grep_pattern() {
    let p = ParsedCommand::parse("cargo test | grep FAIL").unwrap();
    assert_eq!(
        p.strip_simple_pipe(),
        Some(StrippedPipe {
            base: "cargo test".to_string(),
            suffix: "grep FAIL".to_string(),
        })
    );
}

#[test]
fn no_strip_multi_pipe() {
    let p = ParsedCommand::parse("cmd | grep foo | tail -5").unwrap();
    assert!(p.strip_simple_pipe().is_none());
}

#[test]
fn no_strip_wc() {
    let p = ParsedCommand::parse("cargo test | wc -l").unwrap();
    assert!(p.strip_simple_pipe().is_none());
}

// --- env_prefix ---

#[test]
fn env_prefix_single() {
    let p = ParsedCommand::parse("FOO=bar git status").unwrap();
    let (prefix, rest) = p.env_prefix().unwrap();
    assert_eq!(prefix, "FOO=bar ");
    assert_eq!(rest, "git status");
}

#[test]
fn env_prefix_multiple() {
    let p = ParsedCommand::parse("A=1 B=2 cargo test").unwrap();
    let (prefix, rest) = p.env_prefix().unwrap();
    assert_eq!(prefix, "A=1 B=2 ");
    assert_eq!(rest, "cargo test");
}

#[test]
fn env_prefix_none_for_plain() {
    let p = ParsedCommand::parse("git status").unwrap();
    assert!(p.env_prefix().is_none());
}

// --- heredoc ---

#[test]
fn toplevel_heredoc() {
    let p = ParsedCommand::parse("cat <<EOF\nhello\nEOF").unwrap();
    assert!(p.has_toplevel_heredoc());
}

#[test]
fn no_heredoc_plain() {
    let p = ParsedCommand::parse("git status").unwrap();
    assert!(!p.has_toplevel_heredoc());
}

// --- has_substitution_heredoc ---

#[test]
fn substitution_heredoc_basic() {
    assert!(has_substitution_heredoc(
        "git commit -m \"$(cat <<'EOF'\nmsg\nEOF\n)\""
    ));
}

#[test]
fn substitution_heredoc_in_compound() {
    assert!(has_substitution_heredoc(
        "git add a && git commit -m \"$(cat <<'EOF'\nmsg\nEOF\n)\" && git push"
    ));
}

#[test]
fn no_substitution_heredoc_for_toplevel() {
    // A bare top-level heredoc is caught by has_toplevel_heredoc, not this.
    assert!(!has_substitution_heredoc("cat <<EOF\nhi\nEOF"));
}

#[test]
fn no_substitution_heredoc_plain_command() {
    assert!(!has_substitution_heredoc("git status"));
}

// --- output redirect ---

// Cases that SHOULD detect a top-level output-to-file redirect.

#[test]
fn output_redirect_basic() {
    assert!(has_toplevel_output_redirect("git diff > /tmp/foo.txt"));
}

#[test]
fn output_redirect_append() {
    assert!(has_toplevel_output_redirect("git diff >> /tmp/foo.txt"));
}

#[test]
fn output_redirect_clobber() {
    // `>|` forces overwrite even when noclobber is set.
    assert!(has_toplevel_output_redirect("git diff >| forced.txt"));
}

#[test]
fn output_redirect_stderr_only() {
    assert!(has_toplevel_output_redirect("git diff 2> errors.log"));
}

#[test]
fn output_redirect_explicit_stdout_fd() {
    assert!(has_toplevel_output_redirect("git diff 1> out.log"));
}

#[test]
fn output_redirect_combined_bash() {
    // bash extension: redirect both stdout and stderr to a file.
    assert!(has_toplevel_output_redirect("git diff &> all.log"));
}

#[test]
fn output_redirect_combined_append() {
    assert!(has_toplevel_output_redirect("git diff &>> append.log"));
}

#[test]
fn output_redirect_to_dev_null() {
    // /dev/null is still a file from the AST's perspective. The agent
    // explicitly suppressed output, so tokf has nothing useful to add.
    assert!(has_toplevel_output_redirect("git status > /dev/null"));
}

#[test]
fn output_redirect_with_fd_merge_after() {
    // First redirect (`> test.log`) writes to a file; the trailing `2>&1`
    // is a fd merge. The presence of the file write is sufficient.
    assert!(has_toplevel_output_redirect("cargo test > test.log 2>&1"));
}

#[test]
fn output_redirect_read_write() {
    // `<>` opens a file for read+write — still writes to a file.
    assert!(has_toplevel_output_redirect("exec 3<> /tmp/sock"));
}

#[test]
fn output_redirect_in_pipeline() {
    // The first command in the pipeline has the redirect.
    assert!(has_toplevel_output_redirect("git diff > foo.txt | head"));
}

#[test]
fn output_redirect_subshell_outer() {
    // Redirect on the Subshell node itself: `Subshell.redirects`.
    assert!(has_toplevel_output_redirect("(git diff) > foo.txt"));
}

#[test]
fn output_redirect_subshell_inner() {
    // Redirect on the inner Command inside Subshell.body.
    assert!(has_toplevel_output_redirect("(git diff > foo.txt)"));
}

#[test]
fn output_redirect_brace_group() {
    assert!(has_toplevel_output_redirect("{ git diff; } > foo.txt"));
}

#[test]
fn output_redirect_amp_with_file_target() {
    // Bash extension `cmd >& filename` (target is a non-digit word) is a
    // file write — distinct from `cmd >&1` (target is digit, fd merge).
    // This exercises the `op == ">&"` && !is_fd_word(target) branch in
    // `is_file_output_op`.
    assert!(has_toplevel_output_redirect("git diff >& all.log"));
}

#[test]
fn output_redirect_if_compound() {
    // Compound `if` construct with its own redirect on the whole block.
    // Exercises the `NodeKind::If { redirects }` arm of the walker.
    assert!(has_toplevel_output_redirect(
        "if true; then git diff; fi > foo.txt"
    ));
}

#[test]
fn output_redirect_while_compound() {
    // Exercises the `NodeKind::While { redirects }` arm.
    assert!(has_toplevel_output_redirect(
        "while true; do git diff; done > foo.txt"
    ));
}

#[test]
fn output_redirect_for_compound() {
    // Exercises the `NodeKind::For { redirects }` arm.
    assert!(has_toplevel_output_redirect(
        "for f in a b; do echo $f; done > out.txt"
    ));
}

// Cases that SHOULD NOT detect — filtering should still apply.

#[test]
fn no_redirect_plain_command() {
    assert!(!has_toplevel_output_redirect("git status"));
}

#[test]
fn no_redirect_fd_merge_2to1() {
    // `2>&1` is fd-to-fd merge; no file involved.
    assert!(!has_toplevel_output_redirect("git diff 2>&1"));
}

#[test]
fn no_redirect_fd_merge_1to2() {
    assert!(!has_toplevel_output_redirect("git diff 1>&2"));
}

#[test]
fn no_redirect_close_fd_stdin() {
    // `>&-` closes a file descriptor; no file write.
    assert!(!has_toplevel_output_redirect("git diff >&-"));
}

#[test]
fn no_redirect_close_fd_stderr() {
    // rable normalises both `>&-` and `2>&-` to op == ">&-".
    assert!(!has_toplevel_output_redirect("git diff 2>&-"));
}

#[test]
fn no_redirect_input() {
    assert!(!has_toplevel_output_redirect("git apply < input.patch"));
}

#[test]
fn no_redirect_herestring() {
    // `<<<` is a herestring (input). Already not a `>` operator.
    assert!(!has_toplevel_output_redirect(r#"grep foo <<< "input""#));
}

#[test]
fn no_redirect_in_double_quotes() {
    // `>` inside a quoted string is literal text, not a redirect operator.
    assert!(!has_toplevel_output_redirect(r#"echo "git diff > foo""#));
}

#[test]
fn no_redirect_in_single_quotes() {
    assert!(!has_toplevel_output_redirect("echo 'cmd > foo'"));
}

#[test]
fn no_redirect_in_substitution() {
    // The redirect is inside `$(...)`. The outer `echo` has no redirect,
    // and tokf must not skip the outer command on the basis of an inner
    // substitution's redirect.
    assert!(!has_toplevel_output_redirect("echo $(git diff > /tmp/foo)"));
}

#[test]
fn no_redirect_in_function_def() {
    // Function definition: body redirects must NOT count, because the
    // definition produces no output. The body runs at call time, where
    // the call site itself is what we want to (potentially) skip.
    assert!(!has_toplevel_output_redirect("foo() { git diff > x; }"));
}

#[test]
fn no_redirect_pipe_to_tee() {
    // `tee` is a command argument, not a redirect operator. Whether to
    // skip in this case is an open question (deferred follow-up); for now
    // we leave the existing pipe-stripping behaviour intact.
    assert!(!has_toplevel_output_redirect("git diff | tee log.txt"));
}

#[test]
fn no_redirect_compound_list_at_top_level() {
    // Locks in the asymmetry vs heredoc: the whole-command check at
    // mod.rs:280 must return false for a compound containing a redirected
    // segment, so that the per-segment loop at mod.rs:307-321 can skip
    // only the offending segment instead of the whole compound.
    assert!(!has_toplevel_output_redirect(
        "git diff > foo.txt; git status"
    ));
}

// --- command_words ---

#[test]
fn basic_command_words() {
    let p = ParsedCommand::parse("git status --short").unwrap();
    let words = p.command_words().unwrap();
    assert_eq!(words, vec!["git", "status", "--short"]);
}

#[test]
fn command_words_with_env() {
    let p = ParsedCommand::parse("FOO=bar git commit -m 'msg'").unwrap();
    let words = p.command_words().unwrap();
    assert_eq!(words[0], "git");
    assert_eq!(words[1], "commit");
}
