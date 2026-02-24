use crate::filter::section::SectionData;

use super::*;

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn sections_with(name: &str, items: Vec<&str>) -> SectionMap {
    let mut map = SectionMap::new();
    map.insert(
        name.to_string(),
        SectionData {
            lines: items.into_iter().map(String::from).collect(),
            blocks: Vec::new(),
        },
    );
    map
}

fn sections_with_blocks(name: &str, blocks: Vec<&str>) -> SectionMap {
    let mut map = SectionMap::new();
    map.insert(
        name.to_string(),
        SectionData {
            lines: Vec::new(),
            blocks: blocks.into_iter().map(String::from).collect(),
        },
    );
    map
}

#[test]
fn simple_variable_substitution() {
    let v = vars(&[("name", "world")]);
    assert_eq!(
        render_template("hello {name}!", &v, &SectionMap::new()),
        "hello world!"
    );
}

#[test]
fn unknown_variable_empty_string() {
    let v = HashMap::new();
    assert_eq!(
        render_template("hello {unknown}!", &v, &SectionMap::new()),
        "hello !"
    );
}

#[test]
fn property_access_count() {
    let s = sections_with("items", vec!["a", "b", "c"]);
    assert_eq!(
        render_template("count: {items.count}", &HashMap::new(), &s),
        "count: 3"
    );
}

#[test]
fn join_with_separator() {
    let s = sections_with("lines", vec!["a", "b", "c"]);
    assert_eq!(
        render_template("{lines | join: \", \"}", &HashMap::new(), &s),
        "a, b, c"
    );
}

#[test]
fn join_with_newline() {
    let s = sections_with("lines", vec!["a", "b"]);
    assert_eq!(
        render_template("{lines | join: \"\\n\"}", &HashMap::new(), &s),
        "a\nb"
    );
}

#[test]
fn each_with_index_and_value() {
    let s = sections_with("items", vec!["foo", "bar"]);
    assert_eq!(
        render_template(
            "{items | each: \"{index}. {value}\" | join: \", \"}",
            &HashMap::new(),
            &s
        ),
        "1. foo, 2. bar"
    );
}

#[test]
fn each_with_truncate_nested() {
    let s = sections_with_blocks("blocks", vec!["short", "this is a rather long string"]);
    assert_eq!(
        render_template(
            "{blocks | each: \"{value | truncate: 10}\" | join: \"; \"}",
            &HashMap::new(),
            &s
        ),
        "short; this is a ...",
    );
}

#[test]
fn truncate_short_string_unchanged() {
    let v = vars(&[("msg", "short")]);
    assert_eq!(
        render_template("{msg | truncate: 100}", &v, &SectionMap::new()),
        "short"
    );
}

#[test]
fn truncate_long_string_truncated() {
    let v = vars(&[("msg", "abcdefghij")]);
    assert_eq!(
        render_template("{msg | truncate: 5}", &v, &SectionMap::new()),
        "abcde..."
    );
}

#[test]
fn full_pipe_chain_each_then_join() {
    let s = sections_with("names", vec!["alice", "bob"]);
    assert_eq!(
        render_template(
            "{names | each: \"- {value}\" | join: \"\\n\"}",
            &HashMap::new(),
            &s
        ),
        "- alice\n- bob"
    );
}

#[test]
fn no_expressions_passthrough() {
    assert_eq!(
        render_template("just text", &HashMap::new(), &SectionMap::new()),
        "just text"
    );
}

#[test]
fn mixed_vars_and_sections() {
    let v = vars(&[("passed", "20"), ("suites", "3")]);
    let s = sections_with("lines", vec!["a", "b"]);
    assert_eq!(
        render_template(
            "{passed} passed ({suites} suites), {lines.count} lines",
            &v,
            &s
        ),
        "20 passed (3 suites), 2 lines"
    );
}

#[test]
fn empty_collection_empty_string() {
    let s = sections_with("items", vec![]);
    assert_eq!(
        render_template("{items | join: \", \"}", &HashMap::new(), &s),
        ""
    );
}

#[test]
fn cargo_test_success_template() {
    let v = vars(&[("passed", "20"), ("suites", "3")]);
    let template = "\u{2713} cargo test: {passed} passed ({suites} suites)";
    assert_eq!(
        render_template(template, &v, &SectionMap::new()),
        "\u{2713} cargo test: 20 passed (3 suites)"
    );
}

#[test]
fn cargo_test_failure_template() {
    let mut sections = SectionMap::new();
    sections.insert(
        "failure_blocks".to_string(),
        SectionData {
            lines: Vec::new(),
            blocks: vec![
                "thread panicked at tests/a.rs".to_string(),
                "thread panicked at tests/b.rs".to_string(),
            ],
        },
    );
    sections.insert(
        "summary_lines".to_string(),
        SectionData {
            lines: vec!["test result: FAILED. 1 passed; 2 failed".to_string()],
            blocks: Vec::new(),
        },
    );

    let template = "FAILURES ({failure_blocks.count}):\n{failure_blocks | each: \"{index}. {value | truncate: 200}\" | join: \"\\n\"}\n\n{summary_lines | join: \"\\n\"}";
    let result = render_template(template, &HashMap::new(), &sections);
    assert!(result.starts_with("FAILURES (2):"));
    assert!(result.contains("1. thread panicked at tests/a.rs"));
    assert!(result.contains("2. thread panicked at tests/b.rs"));
    assert!(result.contains("test result: FAILED. 1 passed; 2 failed"));
}

#[test]
fn nested_brace_handling() {
    let v = vars(&[("a", "1"), ("b", "2")]);
    assert_eq!(
        render_template("{a}+{b}=3", &v, &SectionMap::new()),
        "1+2=3"
    );
}

#[test]
fn unescape_escaped_quote() {
    assert_eq!(super::unescape(r#"say \"hello\""#), "say \"hello\"");
}

// --- Gap 5: lines, keep, where pipes ---

#[test]
fn pipe_lines_splits_string() {
    let v = vars(&[("msg", "a\nb\nc")]);
    // lines splits into a collection; join reassembles
    let result = render_template("{msg | lines | join: \",\"}", &v, &SectionMap::new());
    assert_eq!(result, "a,b,c");
}

#[test]
fn pipe_lines_on_collection_passthrough() {
    let s = sections_with("items", vec!["x", "y"]);
    // Already a collection → lines is a no-op
    let result = render_template("{items | lines | join: \",\"}", &HashMap::new(), &s);
    assert_eq!(result, "x,y");
}

#[test]
fn pipe_keep_filters_collection() {
    let s = sections_with("lines", vec!["ok line", "error: bad", "ok again"]);
    let result = render_template(
        "{lines | keep: \"^error\" | join: \"||\"}",
        &HashMap::new(),
        &s,
    );
    assert_eq!(result, "error: bad");
}

#[test]
fn pipe_where_is_alias_for_keep() {
    let s = sections_with("lines", vec!["ok line", "error: bad", "ok again"]);
    let result = render_template(
        "{lines | where: \"^error\" | join: \"||\"}",
        &HashMap::new(),
        &s,
    );
    assert_eq!(result, "error: bad");
}

#[test]
fn pipe_keep_no_match_returns_empty() {
    let s = sections_with("lines", vec!["foo", "bar"]);
    let result = render_template(
        "{lines | keep: \"^NOMATCH\" | join: \",\"}",
        &HashMap::new(),
        &s,
    );
    assert_eq!(result, "");
}

#[test]
fn pipe_keep_invalid_regex_passthrough() {
    let s = sections_with("lines", vec!["a", "b"]);
    // Bad regex → value passes through as-is (collection)
    let result = render_template(
        "{lines | keep: \"[invalid\" | join: \",\"}",
        &HashMap::new(),
        &s,
    );
    assert_eq!(result, "a,b");
}

#[test]
fn pipe_lines_then_keep_chain() {
    let v = vars(&[("log", "ok\nfail\nok")]);
    let result = render_template(
        "{log | lines | keep: \"fail\" | join: \",\"}",
        &v,
        &SectionMap::new(),
    );
    assert_eq!(result, "fail");
}

#[test]
fn pipe_lines_then_keep_then_join_chain() {
    let v = vars(&[("log", "pass\nERROR: bad\npass")]);
    let result = render_template(
        "{log | lines | keep: \"^ERROR\" | join: \"\\n\"}",
        &v,
        &SectionMap::new(),
    );
    assert_eq!(result, "ERROR: bad");
}
