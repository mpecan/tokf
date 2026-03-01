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
        render_template("hello {name}!", &v, &SectionMap::new(), &ChunkMap::new()),
        "hello world!"
    );
}

#[test]
fn unknown_variable_empty_string() {
    let v = HashMap::new();
    assert_eq!(
        render_template("hello {unknown}!", &v, &SectionMap::new(), &ChunkMap::new()),
        "hello !"
    );
}

#[test]
fn property_access_count() {
    let s = sections_with("items", vec!["a", "b", "c"]);
    assert_eq!(
        render_template(
            "count: {items.count}",
            &HashMap::new(),
            &s,
            &ChunkMap::new()
        ),
        "count: 3"
    );
}

#[test]
fn join_with_separator() {
    let s = sections_with("lines", vec!["a", "b", "c"]);
    assert_eq!(
        render_template(
            "{lines | join: \", \"}",
            &HashMap::new(),
            &s,
            &ChunkMap::new()
        ),
        "a, b, c"
    );
}

#[test]
fn join_with_newline() {
    let s = sections_with("lines", vec!["a", "b"]);
    assert_eq!(
        render_template(
            "{lines | join: \"\\n\"}",
            &HashMap::new(),
            &s,
            &ChunkMap::new()
        ),
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
            &s,
            &ChunkMap::new(),
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
            &s,
            &ChunkMap::new(),
        ),
        "short; this is a ...",
    );
}

#[test]
fn truncate_short_string_unchanged() {
    let v = vars(&[("msg", "short")]);
    assert_eq!(
        render_template(
            "{msg | truncate: 100}",
            &v,
            &SectionMap::new(),
            &ChunkMap::new()
        ),
        "short"
    );
}

#[test]
fn truncate_long_string_truncated() {
    let v = vars(&[("msg", "abcdefghij")]);
    assert_eq!(
        render_template(
            "{msg | truncate: 5}",
            &v,
            &SectionMap::new(),
            &ChunkMap::new()
        ),
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
            &s,
            &ChunkMap::new(),
        ),
        "- alice\n- bob"
    );
}

#[test]
fn no_expressions_passthrough() {
    assert_eq!(
        render_template(
            "just text",
            &HashMap::new(),
            &SectionMap::new(),
            &ChunkMap::new()
        ),
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
            &s,
            &ChunkMap::new(),
        ),
        "20 passed (3 suites), 2 lines"
    );
}

#[test]
fn empty_collection_empty_string() {
    let s = sections_with("items", vec![]);
    assert_eq!(
        render_template(
            "{items | join: \", \"}",
            &HashMap::new(),
            &s,
            &ChunkMap::new()
        ),
        ""
    );
}

#[test]
fn cargo_test_success_template() {
    let v = vars(&[("passed", "20"), ("suites", "3")]);
    let template = "\u{2713} cargo test: {passed} passed ({suites} suites)";
    assert_eq!(
        render_template(template, &v, &SectionMap::new(), &ChunkMap::new()),
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
    let result = render_template(template, &HashMap::new(), &sections, &ChunkMap::new());
    assert!(result.starts_with("FAILURES (2):"));
    assert!(result.contains("1. thread panicked at tests/a.rs"));
    assert!(result.contains("2. thread panicked at tests/b.rs"));
    assert!(result.contains("test result: FAILED. 1 passed; 2 failed"));
}

#[test]
fn nested_brace_handling() {
    let v = vars(&[("a", "1"), ("b", "2")]);
    assert_eq!(
        render_template("{a}+{b}=3", &v, &SectionMap::new(), &ChunkMap::new()),
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
    let result = render_template(
        "{msg | lines | join: \",\"}",
        &v,
        &SectionMap::new(),
        &ChunkMap::new(),
    );
    assert_eq!(result, "a,b,c");
}

#[test]
fn pipe_lines_on_collection_passthrough() {
    let s = sections_with("items", vec!["x", "y"]);
    // Already a collection → lines is a no-op
    let result = render_template(
        "{items | lines | join: \",\"}",
        &HashMap::new(),
        &s,
        &ChunkMap::new(),
    );
    assert_eq!(result, "x,y");
}

#[test]
fn pipe_keep_filters_collection() {
    let s = sections_with("lines", vec!["ok line", "error: bad", "ok again"]);
    let result = render_template(
        "{lines | keep: \"^error\" | join: \"||\"}",
        &HashMap::new(),
        &s,
        &ChunkMap::new(),
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
        &ChunkMap::new(),
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
        &ChunkMap::new(),
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
        &ChunkMap::new(),
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
        &ChunkMap::new(),
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
        &ChunkMap::new(),
    );
    assert_eq!(result, "ERROR: bad");
}

// --- Structured collection (chunk) tests ---

use super::super::chunk::ChunkData;

fn chunks_with(name: &str, items: Vec<Vec<(&str, &str)>>) -> ChunkMap {
    let mut map = ChunkMap::new();
    map.insert(
        name.to_string(),
        ChunkData::Flat(
            items
                .into_iter()
                .map(|pairs| {
                    pairs
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect()
                })
                .collect(),
        ),
    );
    map
}

#[allow(clippy::type_complexity)]
fn tree_chunks_with(
    name: &str,
    groups: Vec<Vec<(&str, &str)>>,
    children_key: &str,
    children: Vec<Vec<Vec<(&str, &str)>>>,
) -> ChunkMap {
    let mut map = ChunkMap::new();
    let groups_items: Vec<ChunkItem> = groups
        .into_iter()
        .map(|pairs| {
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .collect();
    let children_items: Vec<Vec<ChunkItem>> = children
        .into_iter()
        .map(|group_children| {
            group_children
                .into_iter()
                .map(|pairs| {
                    pairs
                        .into_iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect()
                })
                .collect()
        })
        .collect();
    map.insert(
        name.to_string(),
        ChunkData::Tree {
            groups: groups_items,
            children_key: children_key.to_string(),
            children: children_items,
        },
    );
    map
}

#[test]
fn structured_collection_count() {
    let c = chunks_with(
        "suites",
        vec![
            vec![("crate", "tokf"), ("passed", "100")],
            vec![("crate", "tokf-filter"), ("passed", "50")],
        ],
    );
    let result = render_template(
        "{suites.count} suites",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "2 suites");
}

#[test]
fn structured_collection_each_with_fields() {
    let c = chunks_with(
        "suites",
        vec![
            vec![("crate", "tokf"), ("passed", "100")],
            vec![("crate", "tokf-filter"), ("passed", "50")],
        ],
    );
    let result = render_template(
        "{suites | each: \"  {crate}: {passed}\" | join: \"\\n\"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "  tokf: 100\n  tokf-filter: 50");
}

#[test]
fn structured_collection_each_with_index() {
    let c = chunks_with(
        "suites",
        vec![vec![("crate", "tokf")], vec![("crate", "tokf-filter")]],
    );
    let result = render_template(
        "{suites | each: \"{index}. {crate}\" | join: \", \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "1. tokf, 2. tokf-filter");
}

#[test]
fn structured_collection_join_without_each() {
    let c = chunks_with("suites", vec![vec![("crate", "tokf"), ("passed", "5")]]);
    // Without each, join uses the format_chunk_item representation
    let result = render_template(
        "{suites | join: \"; \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    // format_chunk_item sorts keys alphabetically
    assert_eq!(result, "crate=tokf, passed=5");
}

#[test]
fn empty_structured_collection() {
    let c = chunks_with("suites", vec![]);
    let result = render_template(
        "count={suites.count}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "count=0");
}

#[test]
fn structured_collection_keep_filters_by_format() {
    let c = chunks_with(
        "suites",
        vec![
            vec![("crate", "tokf"), ("passed", "100")],
            vec![("crate", "tokf-filter"), ("passed", "50")],
            vec![("crate", "other"), ("passed", "0")],
        ],
    );
    // keep filters by format_chunk_item representation (key=value pairs)
    let result = render_template(
        "{suites | keep: \"tokf-filter\" | join: \"; \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert!(result.contains("tokf-filter"));
    assert!(!result.contains("other"));
}

#[test]
fn structured_collection_where_alias_for_keep() {
    let c = chunks_with(
        "suites",
        vec![vec![("name", "alpha")], vec![("name", "beta")]],
    );
    let result = render_template(
        "{suites | where: \"alpha\" | join: \", \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert!(result.contains("alpha"));
    assert!(!result.contains("beta"));
}

#[test]
fn structured_collection_truncate_passthrough() {
    // truncate on StructuredCollection passes through unchanged
    let c = chunks_with("suites", vec![vec![("crate", "tokf"), ("passed", "100")]]);
    let result = render_template(
        "{suites | truncate: 5 | join: \", \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    // Should still contain the full item representation (passthrough)
    assert!(result.contains("crate=tokf"));
}

// --- Tree collection tests ---

#[test]
fn tree_collection_count() {
    let c = tree_chunks_with(
        "suites",
        vec![
            vec![("crate", "tokf"), ("passed", "23")],
            vec![("crate", "tokf-filter"), ("passed", "50")],
        ],
        "children",
        vec![
            vec![
                vec![("crate", "tokf"), ("passed", "12")],
                vec![("crate", "tokf"), ("passed", "11")],
            ],
            vec![vec![("crate", "tokf-filter"), ("passed", "50")]],
        ],
    );
    let result = render_template(
        "{suites.count} groups",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "2 groups");
}

#[test]
fn tree_collection_each_with_fields() {
    let c = tree_chunks_with(
        "suites",
        vec![
            vec![("crate", "alpha"), ("passed", "15")],
            vec![("crate", "beta"), ("passed", "5")],
        ],
        "children",
        vec![
            vec![vec![("crate", "alpha"), ("passed", "15")]],
            vec![vec![("crate", "beta"), ("passed", "5")]],
        ],
    );
    let result = render_template(
        "{suites | each: \"  {crate}: {passed}\" | join: \"\\n\"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "  alpha: 15\n  beta: 5");
}

#[test]
fn tree_collection_each_with_children() {
    let c = tree_chunks_with(
        "suites",
        vec![vec![("crate", "tokf"), ("passed", "23")]],
        "children",
        vec![vec![
            vec![("suite", "lib"), ("passed", "12")],
            vec![("suite", "main"), ("passed", "11")],
        ]],
    );
    let result = render_template(
        "{suites | each: \"{crate}: {passed}\\n{children | each: \\\"  {suite}: {passed}\\\" | join: \\\"\\\\n\\\"}\" | join: \"\\n\"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert!(result.contains("tokf: 23"));
    assert!(result.contains("  lib: 12"));
    assert!(result.contains("  main: 11"));
}

#[test]
fn tree_collection_join_without_each() {
    let c = tree_chunks_with(
        "suites",
        vec![vec![("crate", "tokf"), ("passed", "5")]],
        "children",
        vec![vec![vec![("crate", "tokf"), ("passed", "5")]]],
    );
    // join on TreeCollection uses group format_chunk_item
    let result = render_template(
        "{suites | join: \"; \"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert!(result.contains("crate=tokf"));
    assert!(result.contains("passed=5"));
}

#[test]
fn tree_collection_keep_filters_groups() {
    let c = tree_chunks_with(
        "suites",
        vec![
            vec![("crate", "alpha"), ("passed", "10")],
            vec![("crate", "beta"), ("passed", "5")],
        ],
        "children",
        vec![
            vec![vec![("crate", "alpha"), ("passed", "10")]],
            vec![vec![("crate", "beta"), ("passed", "5")]],
        ],
    );
    let result = render_template(
        "{suites | keep: \"alpha\" | each: \"{crate}\" | join: \",\"}",
        &HashMap::new(),
        &SectionMap::new(),
        &c,
    );
    assert_eq!(result, "alpha");
}
