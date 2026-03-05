use super::chunk::{ChunkData, ChunkItem, normalize_keys, process_chunks};
use tokf_common::config::types::{ChunkAggregateRule, ChunkBodyExtract, ChunkConfig, ChunkExtract};

fn basic_config() -> ChunkConfig {
    ChunkConfig {
        split_on: r"^\s*Running ".to_string(),
        include_split_line: true,
        collect_as: "suites".to_string(),
        extract: Some(ChunkExtract {
            pattern: r"deps/([\w_-]+)-".to_string(),
            as_name: "crate".to_string(),
            carry_forward: false,
        }),
        body_extract: vec![],
        aggregate: vec![ChunkAggregateRule {
            pattern: r"(\d+) passed".to_string(),
            sum: Some("passed".to_string()),
            count_as: None,
        }],
        group_by: None,
        children_as: None,
    }
}

/// Helper to unwrap a Flat `ChunkData` into its items.
fn flat_items(data: &ChunkData) -> &Vec<ChunkItem> {
    match data {
        ChunkData::Flat(items) => items,
        ChunkData::Tree { .. } => panic!("expected Flat, got Tree"),
    }
}

#[test]
fn basic_chunk_split_and_extract() {
    let lines = vec![
        "   Compiling tokf v0.1.0",
        "     Running unittests src/lib.rs (target/debug/deps/tokf_filter-abc123)",
        "running 208 tests",
        "test result: ok. 208 passed; 0 failed; 1 ignored",
        "     Running unittests src/lib.rs (target/debug/deps/tokf_server-def456)",
        "running 105 tests",
        "test result: ok. 105 passed; 0 failed; 0 ignored",
    ];
    let config = basic_config();
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 2);
    assert_eq!(suites[0]["crate"], "tokf_filter");
    assert_eq!(suites[0]["passed"], "208");
    assert_eq!(suites[1]["crate"], "tokf_server");
    assert_eq!(suites[1]["passed"], "105");
}

#[test]
fn group_by_merges_same_crate() {
    let lines = vec![
        "     Running unittests src/lib.rs (target/debug/deps/tokf_filter-abc123)",
        "test result: ok. 100 passed; 0 failed; 0 ignored",
        "     Running tests/integration.rs (target/debug/deps/tokf_filter-def456)",
        "test result: ok. 50 passed; 0 failed; 2 ignored",
    ];
    let mut config = basic_config();
    config.group_by = Some("crate".to_string());
    config.aggregate.push(ChunkAggregateRule {
        pattern: r"(\d+) ignored".to_string(),
        sum: Some("ignored".to_string()),
        count_as: None,
    });
    config.aggregate.push(ChunkAggregateRule {
        pattern: r"^test result:".to_string(),
        sum: None,
        count_as: Some("suite_count".to_string()),
    });
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1); // grouped into one
    assert_eq!(suites[0]["crate"], "tokf_filter");
    assert_eq!(suites[0]["passed"], "150"); // 100 + 50
    assert_eq!(suites[0]["ignored"], "2"); // 0 + 2
    assert_eq!(suites[0]["suite_count"], "2"); // 1 + 1
}

#[test]
fn exclude_header_line() {
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 10 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.include_split_line = false;
    // extract won't find the header since it's excluded
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["crate"], ""); // header was excluded, seeded from config as empty
    assert_eq!(suites[0]["passed"], "10");
}

#[test]
fn body_extract() {
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "running 42 tests",
        "test result: ok. 42 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.body_extract = vec![ChunkBodyExtract {
        pattern: r"running (\d+) tests".to_string(),
        as_name: "total_tests".to_string(),
        carry_forward: false,
    }];
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites[0]["total_tests"], "42");
}

#[test]
fn empty_input() {
    let config = basic_config();
    let result = process_chunks(&[config], &[]);
    assert!(flat_items(&result["suites"]).is_empty());
}

#[test]
fn no_matches() {
    let lines = vec!["no matching lines here", "just noise"];
    let config = basic_config();
    let result = process_chunks(&[config], &lines);
    assert!(flat_items(&result["suites"]).is_empty());
}

#[test]
fn invalid_split_regex_skipped() {
    let config = ChunkConfig {
        split_on: "[invalid".to_string(),
        include_split_line: true,
        collect_as: "bad".to_string(),
        extract: None,
        body_extract: vec![],
        aggregate: vec![],
        group_by: None,
        children_as: None,
    };
    let result = process_chunks(&[config], &["line1", "line2"]);
    assert!(!result.contains_key("bad"));
}

#[test]
fn lines_before_first_match_discarded() {
    let lines = vec![
        "preamble line 1",
        "preamble line 2",
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 5 passed; 0 failed",
    ];
    let config = basic_config();
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["passed"], "5");
}

#[test]
fn multiple_aggregate_rules_per_chunk() {
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 10 passed; 2 failed; 3 ignored",
    ];
    let config = ChunkConfig {
        split_on: r"^\s*Running ".to_string(),
        include_split_line: true,
        collect_as: "suites".to_string(),
        extract: None,
        body_extract: vec![],
        aggregate: vec![
            ChunkAggregateRule {
                pattern: r"(\d+) passed".to_string(),
                sum: Some("passed".to_string()),
                count_as: None,
            },
            ChunkAggregateRule {
                pattern: r"(\d+) failed".to_string(),
                sum: Some("failed".to_string()),
                count_as: None,
            },
            ChunkAggregateRule {
                pattern: r"(\d+) ignored".to_string(),
                sum: Some("ignored".to_string()),
                count_as: None,
            },
        ],
        group_by: None,
        children_as: None,
    };
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites[0]["passed"], "10");
    assert_eq!(suites[0]["failed"], "2");
    assert_eq!(suites[0]["ignored"], "3");
}

#[test]
fn normalize_keys_fills_missing_fields() {
    let mut items = vec![
        ChunkItem::from([
            ("crate".to_string(), "alpha".to_string()),
            ("passed".to_string(), "10".to_string()),
        ]),
        ChunkItem::from([
            ("crate".to_string(), "beta".to_string()),
            // missing "passed"
        ]),
    ];
    normalize_keys(&basic_config(), &mut items);
    assert_eq!(items[0]["passed"], "10");
    assert_eq!(items[1]["passed"], ""); // filled with empty string
    assert_eq!(items[0]["crate"], "alpha");
    assert_eq!(items[1]["crate"], "beta");
}

#[test]
fn normalize_keys_no_items() {
    let mut items: Vec<ChunkItem> = vec![];
    normalize_keys(&basic_config(), &mut items); // should not panic
    assert!(items.is_empty());
}

#[test]
fn normalize_keys_uniform_items_unchanged() {
    let mut items = vec![
        ChunkItem::from([
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]),
        ChunkItem::from([
            ("a".to_string(), "3".to_string()),
            ("b".to_string(), "4".to_string()),
        ]),
    ];
    normalize_keys(&basic_config(), &mut items);
    assert_eq!(items[0]["a"], "1");
    assert_eq!(items[0]["b"], "2");
    assert_eq!(items[1]["a"], "3");
    assert_eq!(items[1]["b"], "4");
}

#[test]
fn normalize_keys_seeds_from_config() {
    // Even when no item has a configured field, it should be seeded from config.
    let mut items = vec![ChunkItem::from([("other".to_string(), "x".to_string())])];
    normalize_keys(&basic_config(), &mut items);
    // basic_config has extract.as = "crate" and aggregate.sum = "passed"
    assert_eq!(items[0].get("crate").unwrap(), "");
    assert_eq!(items[0].get("passed").unwrap(), "");
}

#[test]
fn chunks_produce_normalized_keys() {
    // One chunk has a matching header, the other doesn't → extract field
    // is only present in one item. normalize_keys should fill the gap.
    let lines = vec![
        "     Running unittests src/lib.rs (target/debug/deps/tokf_filter-abc123)",
        "test result: ok. 10 passed; 0 failed",
        "     Running some-other-format-without-deps-path",
        "test result: ok. 5 passed; 0 failed",
    ];
    let config = basic_config();
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 2);
    // First chunk has "crate" extracted, second does not
    assert_eq!(suites[0]["crate"], "tokf_filter");
    // Second chunk should have empty "crate" (normalized), not missing
    assert!(suites[1].contains_key("crate"));
    assert_eq!(suites[1]["crate"], "");
}

#[test]
fn single_chunk_boundary() {
    // Only one split match → one chunk
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 42 passed; 0 failed",
    ];
    let config = basic_config();
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["crate"], "tokf_filter");
    assert_eq!(suites[0]["passed"], "42");
}

#[test]
fn group_by_no_matching_field() {
    // group_by references a field that no item has → all merge under empty key
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running deps/tokf_server-def456",
        "test result: ok. 5 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.group_by = Some("nonexistent".to_string());
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    // Both items have nonexistent="" (from normalize_keys), so they merge
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["passed"], "15"); // 10 + 5
}

#[test]
fn invalid_extract_regex_skipped() {
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "test result: ok. 10 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.extract = Some(ChunkExtract {
        pattern: "[invalid".to_string(),
        as_name: "name".to_string(),
        carry_forward: false,
    });
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    // Extract regex invalid — field seeded from config as empty
    assert_eq!(suites[0]["name"], "");
    assert_eq!(suites[0]["passed"], "10"); // aggregation still works
}

#[test]
fn invalid_body_extract_regex_skipped() {
    let lines = vec![
        "     Running deps/tokf_filter-abc123",
        "running 42 tests",
        "test result: ok. 42 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.body_extract = vec![ChunkBodyExtract {
        pattern: "[invalid".to_string(),
        as_name: "total".to_string(),
        carry_forward: false,
    }];
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["total"], ""); // regex invalid, seeded from config as empty
    assert_eq!(suites[0]["passed"], "42");
}

#[test]
fn multiple_chunk_configs() {
    let lines = vec![
        "--- alpha",
        "10 items",
        "--- beta",
        "5 items",
        "=== X",
        "hello",
        "=== Y",
        "world",
    ];
    let config1 = ChunkConfig {
        split_on: "^--- ".to_string(),
        include_split_line: true,
        collect_as: "dashes".to_string(),
        extract: Some(ChunkExtract {
            pattern: r"--- (\w+)".to_string(),
            as_name: "name".to_string(),
            carry_forward: false,
        }),
        body_extract: vec![],
        aggregate: vec![ChunkAggregateRule {
            pattern: r"(\d+) items".to_string(),
            sum: Some("count".to_string()),
            count_as: None,
        }],
        group_by: None,
        children_as: None,
    };
    let config2 = ChunkConfig {
        split_on: "^=== ".to_string(),
        include_split_line: true,
        collect_as: "equals".to_string(),
        extract: Some(ChunkExtract {
            pattern: r"=== (\w+)".to_string(),
            as_name: "label".to_string(),
            carry_forward: false,
        }),
        body_extract: vec![],
        aggregate: vec![],
        group_by: None,
        children_as: None,
    };
    let result = process_chunks(&[config1, config2], &lines);
    let dashes = flat_items(&result["dashes"]);
    assert_eq!(dashes.len(), 2);
    assert_eq!(dashes[0]["name"], "alpha");
    assert_eq!(dashes[0]["count"], "10");
    let equals = flat_items(&result["equals"]);
    assert_eq!(equals.len(), 2);
    assert_eq!(equals[0]["label"], "X");
    assert_eq!(equals[1]["label"], "Y");
}

// --- carry_forward tests ---

#[test]
fn carry_forward_fills_missing_extract() {
    let lines = vec![
        "     Running deps/tokf-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running some-other-no-deps-path",
        "test result: ok. 5 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.extract = Some(ChunkExtract {
        pattern: r"deps/([\w_-]+)-".to_string(),
        as_name: "crate".to_string(),
        carry_forward: true,
    });
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 2);
    assert_eq!(suites[0]["crate"], "tokf");
    // Second chunk inherits crate from first via carry_forward
    assert_eq!(suites[1]["crate"], "tokf");
}

#[test]
fn carry_forward_updates_on_new_match() {
    let lines = vec![
        "     Running deps/alpha-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running no-deps",
        "test result: ok. 5 passed; 0 failed",
        "     Running deps/beta-def456",
        "test result: ok. 3 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.extract = Some(ChunkExtract {
        pattern: r"deps/([\w_-]+)-".to_string(),
        as_name: "crate".to_string(),
        carry_forward: true,
    });
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites[0]["crate"], "alpha");
    assert_eq!(suites[1]["crate"], "alpha"); // carried from first
    assert_eq!(suites[2]["crate"], "beta"); // new match overrides
}

#[test]
fn carry_forward_disabled_by_default() {
    let lines = vec![
        "     Running deps/tokf-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running no-deps",
        "test result: ok. 5 passed; 0 failed",
    ];
    let config = basic_config(); // carry_forward = false
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites[0]["crate"], "tokf");
    assert_eq!(suites[1]["crate"], ""); // NOT carried — normalized to empty
}

// --- merge_into tests ---

#[test]
fn merge_into_empty_existing_with_numeric_incoming() {
    // When the first chunk in a group has an empty field (from normalize_keys)
    // and a later chunk has a numeric value, the group should pick up the number.
    let lines = vec![
        "     Running no-deps-match",
        "test result: ok. 0 passed; 0 failed",
        "     Running deps/tokf-abc123",
        "test result: ok. 10 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.group_by = Some("crate".to_string());
    config.extract = Some(ChunkExtract {
        pattern: r"deps/([\w_-]+)-".to_string(),
        as_name: "crate".to_string(),
        carry_forward: false,
    });
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    // The first chunk has crate="" and the second has crate="tokf",
    // so they should NOT merge. But verify the empty-crate group still sums.
    assert_eq!(suites.len(), 2);
    assert_eq!(suites[0]["crate"], "");
    assert_eq!(suites[1]["crate"], "tokf");
    assert_eq!(suites[1]["passed"], "10");
}

// --- group_by_field_with_children tests ---

#[test]
fn group_by_with_children_preserves_items() {
    let lines = vec![
        "     Running deps/tokf-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running deps/tokf-def456",
        "test result: ok. 5 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.group_by = Some("crate".to_string());
    config.children_as = Some("children".to_string());
    let result = process_chunks(&[config], &lines);
    match &result["suites"] {
        ChunkData::Tree {
            groups,
            children_key,
            children,
        } => {
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0]["crate"], "tokf");
            assert_eq!(groups[0]["passed"], "15"); // 10 + 5
            assert_eq!(children_key, "children");
            assert_eq!(children.len(), 1); // one group
            assert_eq!(children[0].len(), 2); // two original items
            assert_eq!(children[0][0]["passed"], "10");
            assert_eq!(children[0][1]["passed"], "5");
        }
        ChunkData::Flat(_) => panic!("expected Tree"),
    }
}

#[test]
fn group_by_with_children_single_group() {
    let lines = vec![
        "     Running deps/alpha-abc123",
        "test result: ok. 10 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.group_by = Some("crate".to_string());
    config.children_as = Some("kids".to_string());
    let result = process_chunks(&[config], &lines);
    match &result["suites"] {
        ChunkData::Tree {
            groups, children, ..
        } => {
            assert_eq!(groups.len(), 1);
            assert_eq!(children[0].len(), 1); // single child
        }
        ChunkData::Flat(_) => panic!("expected Tree"),
    }
}

#[test]
fn group_by_without_children_as_remains_flat() {
    let lines = vec![
        "     Running deps/tokf-abc123",
        "test result: ok. 10 passed; 0 failed",
        "     Running deps/tokf-def456",
        "test result: ok. 5 passed; 0 failed",
    ];
    let mut config = basic_config();
    config.group_by = Some("crate".to_string());
    // children_as is None
    let result = process_chunks(&[config], &lines);
    let suites = flat_items(&result["suites"]);
    assert_eq!(suites.len(), 1);
    assert_eq!(suites[0]["passed"], "15");
}
