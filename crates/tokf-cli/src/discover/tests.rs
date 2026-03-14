use super::parser::parse_session;
use super::types::CommandAnalysis;
use super::*;

fn make_jsonl_line(json: &str) -> String {
    format!("{json}\n")
}

fn assistant_bash_tool_use(id: &str, command: &str) -> String {
    make_jsonl_line(&format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{"command":"{command}"}}}}]}}}}"#
    ))
}

fn user_tool_result(tool_use_id: &str, text: &str) -> String {
    make_jsonl_line(&format!(
        r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"{text}"}}]}}}}"#
    ))
}

fn progress_bash_tool_use(id: &str, command: &str) -> String {
    make_jsonl_line(&format!(
        r#"{{"type":"progress","data":{{"message":{{"message":{{"content":[{{"type":"tool_use","id":"{id}","name":"Bash","input":{{"command":"{command}"}}}}]}}}}}}}}"#
    ))
}

fn progress_tool_result(tool_use_id: &str, text: &str) -> String {
    make_jsonl_line(&format!(
        r#"{{"type":"progress","data":{{"message":{{"message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":"{text}"}}]}}}}}}}}"#
    ))
}

#[test]
fn parse_top_level_bash_command() {
    let session = format!(
        "{}{}",
        assistant_bash_tool_use("tu_1", "git status"),
        user_tool_result("tu_1", "On branch main"),
    );
    let cmds = parse_session(session.as_bytes());
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "git status");
    assert_eq!(cmds[0].tool_use_id, "tu_1");
    assert_eq!(cmds[0].output_bytes, "On branch main".len());
}

#[test]
fn parse_sub_agent_commands() {
    let session = format!(
        "{}{}",
        progress_bash_tool_use("tu_sub", "cargo test"),
        progress_tool_result("tu_sub", "test result: ok"),
    );
    let cmds = parse_session(session.as_bytes());
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "cargo test");
    assert_eq!(cmds[0].output_bytes, "test result: ok".len());
}

#[test]
fn parse_tool_result_before_tool_use() {
    // Result arrives before the use (can happen with interleaved messages)
    let session = format!(
        "{}{}",
        user_tool_result("tu_late", "output data"),
        assistant_bash_tool_use("tu_late", "echo hello"),
    );
    let cmds = parse_session(session.as_bytes());
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].command, "echo hello");
    assert_eq!(cmds[0].output_bytes, "output data".len());
}

#[test]
fn parse_multiple_commands() {
    let session = format!(
        "{}{}{}{}",
        assistant_bash_tool_use("tu_a", "git status"),
        user_tool_result("tu_a", "clean"),
        assistant_bash_tool_use("tu_b", "cargo build"),
        user_tool_result("tu_b", "Compiling...done"),
    );
    let cmds = parse_session(session.as_bytes());
    assert_eq!(cmds.len(), 2);
}

#[test]
fn parse_skips_non_bash_tools() {
    let session = make_jsonl_line(
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu_x","name":"Read","input":{"path":"/foo"}}]}}"#,
    );
    let cmds = parse_session(session.as_bytes());
    assert!(cmds.is_empty());
}

#[test]
fn parse_skips_malformed_lines() {
    let session = "not json\n{\"broken\n".to_string();
    let cmds = parse_session(session.as_bytes());
    assert!(cmds.is_empty());
}

#[test]
fn parse_empty_session() {
    let cmds = parse_session(b"" as &[u8]);
    assert!(cmds.is_empty());
}

#[test]
fn parse_tool_result_array_content() {
    let session = format!(
        "{}{}",
        assistant_bash_tool_use("tu_arr", "ls"),
        make_jsonl_line(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu_arr","content":[{"type":"text","text":"file1.rs"},{"type":"text","text":"file2.rs"}]}]}}"#,
        ),
    );
    let cmds = parse_session(session.as_bytes());
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].output_bytes, "file1.rs".len() + "file2.rs".len());
}

// --- classify_command tests ---

fn make_cmd(command: &str, output_bytes: usize) -> ExtractedCommand {
    ExtractedCommand {
        tool_use_id: "test".to_string(),
        command: command.to_string(),
        output_bytes,
    }
}

#[test]
fn classify_already_filtered() {
    let cmd = make_cmd("tokf run git status", 100);
    let result = classify_command(&cmd, &[], &[]);
    assert_eq!(result, CommandAnalysis::AlreadyFiltered);
}

#[test]
fn classify_no_filter_empty_list() {
    let cmd = make_cmd("some-unknown-tool", 100);
    let result = classify_command(&cmd, &[], &[]);
    assert_eq!(result, CommandAnalysis::NoFilter);
}

// --- normalize_command tests ---

#[test]
fn normalize_strips_trailing_pipe() {
    assert_eq!(normalize_command("git log | head -20"), "git log");
}

#[test]
fn normalize_strips_grep_pipe() {
    assert_eq!(
        normalize_command("cargo test 2>&1 | grep FAILED"),
        "cargo test 2>&1"
    );
}

#[test]
fn normalize_preserves_no_pipe() {
    assert_eq!(normalize_command("git status"), "git status");
}

#[test]
fn normalize_preserves_or_operator() {
    // `||` should not be treated as a pipe
    assert_eq!(
        normalize_command("test -f foo || echo missing"),
        "test -f foo || echo missing"
    );
}

// --- encode_project_path tests ---

#[test]
fn encode_project_path_basic() {
    let path = std::path::Path::new("/Users/foo/project");
    assert_eq!(encode_project_path(path), "-Users-foo-project");
}

#[test]
fn encode_project_path_trailing_slash() {
    let path = std::path::Path::new("/Users/foo/project/");
    // PathBuf normalizes trailing slash
    assert_eq!(encode_project_path(path), "-Users-foo-project");
}

#[test]
fn encode_project_path_dots_replaced() {
    let path = std::path::Path::new("/home/user/src/github.com/org/repo");
    assert_eq!(
        encode_project_path(path),
        "-home-user-src-github-com-org-repo"
    );
}

// --- command_group_key tests ---

#[test]
fn group_key_simple_command() {
    assert_eq!(command_group_key("find /tmp -name '*.rs'"), "find");
}

#[test]
fn group_key_with_subcommand() {
    assert_eq!(command_group_key("gh pr list --limit 5"), "gh pr list");
}

#[test]
fn group_key_with_flags() {
    assert_eq!(command_group_key("cargo test --workspace"), "cargo test");
}

#[test]
fn group_key_strips_env_vars() {
    assert_eq!(command_group_key("RUST_LOG=debug cargo test"), "cargo test");
}

#[test]
fn group_key_strips_path_prefix() {
    assert_eq!(command_group_key("/usr/bin/find /tmp"), "find");
}

#[test]
fn group_key_stops_at_path_arg() {
    assert_eq!(command_group_key("ls -la /some/path"), "ls");
}

#[test]
fn group_key_max_depth() {
    assert_eq!(
        command_group_key("gh pr checks 123 --watch"),
        "gh pr checks"
    );
}

#[test]
fn group_key_python_subcommand() {
    assert_eq!(
        command_group_key("python manage.py migrate"),
        "python manage.py migrate"
    );
}

#[test]
fn group_key_stops_at_dollar() {
    assert_eq!(command_group_key("echo $HOME"), "echo");
}

// --- extract_group_keys tests ---

#[test]
fn extract_keys_compound_command() {
    let keys = extract_group_keys("cd /tmp && gh repo view foo");
    assert_eq!(keys, vec!["cd", "gh repo view"]);
}

#[test]
fn extract_keys_single_command() {
    let keys = extract_group_keys("cargo test");
    assert_eq!(keys, vec!["cargo test"]);
}

#[test]
fn extract_keys_or_chain() {
    let keys = extract_group_keys("test -f foo || echo missing");
    // "missing" looks like a subcommand to the heuristic — that's fine for grouping
    assert_eq!(keys, vec!["test", "echo missing"]);
}

// --- strip_heredoc tests ---

#[test]
fn strip_heredoc_removes_heredoc() {
    assert_eq!(
        strip_heredoc("git commit -m \"$(cat <<'EOF'"),
        "git commit -m \"$(cat"
    );
}

#[test]
fn strip_heredoc_no_heredoc() {
    assert_eq!(
        strip_heredoc("cargo test --release"),
        "cargo test --release"
    );
}

// --- command_group_key edge cases ---

#[test]
fn group_key_skips_code_fragments() {
    assert_eq!(command_group_key("\"quoted stuff\""), "");
    assert_eq!(command_group_key("(subshell)"), "");
    assert_eq!(command_group_key("{block}"), "");
}

#[test]
fn group_key_skips_pipe_fragments() {
    assert_eq!(command_group_key("s|\\.toml$"), "");
}

#[test]
fn group_key_handles_for_loop() {
    assert_eq!(command_group_key("for f in *.rs"), "for f in");
}

#[test]
fn group_key_stops_at_glob() {
    assert_eq!(command_group_key("wc -l *.rs"), "wc");
}

// --- extract_group_keys with heredoc ---

#[test]
fn extract_keys_strips_heredoc() {
    let keys = extract_group_keys("git commit -m \"$(cat <<'EOF'\nmessage\nEOF\n)\"");
    assert_eq!(keys, vec!["git commit"]);
}

#[test]
fn extract_keys_filters_empty() {
    // A command that produces only non-command fragments
    let keys = extract_group_keys("\"just a string\"");
    assert!(keys.is_empty());
}

// --- discover results grouping ---

#[test]
fn discover_groups_unfiltered_commands() {
    let dir = tempfile::TempDir::new().unwrap();

    // Two different `find` invocations should be grouped together
    let session = format!(
        "{}{}{}{}",
        assistant_bash_tool_use("tu_1", "find /tmp -name '*.rs'"),
        user_tool_result("tu_1", &"x".repeat(400)),
        assistant_bash_tool_use("tu_2", "find /var -type f"),
        user_tool_result("tu_2", &"y".repeat(200)),
    );
    let session_path = dir.path().join("session_group.jsonl");
    std::fs::write(&session_path, &session).unwrap();

    let summary = discover_sessions(&[session_path], true).unwrap();
    // Both `find` commands should be grouped under the "find" key
    let find_results: Vec<_> = summary
        .results
        .iter()
        .filter(|r| r.command_pattern == "find")
        .collect();
    assert_eq!(find_results.len(), 1, "both finds should be grouped");
    assert_eq!(find_results[0].occurrences, 2);
    assert!(!find_results[0].has_filter);
}

// --- aggregation integration test ---

#[test]
fn discover_sessions_with_temp_files() {
    let dir = tempfile::TempDir::new().unwrap();

    // Write a synthetic session file
    let session = format!(
        "{}{}{}{}",
        assistant_bash_tool_use("tu_1", "git status"),
        user_tool_result("tu_1", &"x".repeat(400)),
        assistant_bash_tool_use("tu_2", "git status"),
        user_tool_result("tu_2", &"y".repeat(800)),
    );
    let session_path = dir.path().join("session1.jsonl");
    std::fs::write(&session_path, &session).unwrap();

    let summary = discover_sessions(&[session_path], true).unwrap();
    assert_eq!(summary.sessions_scanned, 1);
    assert_eq!(summary.total_commands, 2);
    // Both should be filterable (git/status is in stdlib)
    assert_eq!(summary.already_filtered, 0);
}

#[test]
fn discover_sessions_counts_already_filtered() {
    let dir = tempfile::TempDir::new().unwrap();

    let session = format!(
        "{}{}",
        assistant_bash_tool_use("tu_1", "tokf run git status"),
        user_tool_result("tu_1", "filtered output"),
    );
    let session_path = dir.path().join("session2.jsonl");
    std::fs::write(&session_path, &session).unwrap();

    let summary = discover_sessions(&[session_path], true).unwrap();
    assert_eq!(summary.already_filtered, 1);
    assert_eq!(summary.filterable_commands, 0);
}
