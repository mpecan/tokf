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
