use std::collections::{HashMap, HashSet};

use regex::Regex;

use tokf_common::config::types::ChunkConfig;

/// One processed chunk's extracted fields (key → string value).
pub type ChunkItem = HashMap<String, String>;

/// Pre-compiled regexes for a single `ChunkConfig`, avoiding per-chunk recompilation.
struct CompiledChunkConfig<'a> {
    config: &'a ChunkConfig,
    extract_re: Option<Regex>,
    body_extract_res: Vec<Option<Regex>>,
    aggregate_res: Vec<Option<Regex>>,
}

impl<'a> CompiledChunkConfig<'a> {
    fn new(config: &'a ChunkConfig) -> Self {
        let extract_re = config
            .extract
            .as_ref()
            .and_then(|e| Regex::new(&e.pattern).ok());
        let body_extract_res = config
            .body_extract
            .iter()
            .map(|be| Regex::new(&be.pattern).ok())
            .collect();
        let aggregate_res = config
            .aggregate
            .iter()
            .map(|a| Regex::new(&a.pattern).ok())
            .collect();
        Self {
            config,
            extract_re,
            body_extract_res,
            aggregate_res,
        }
    }
}

/// Process all chunk configurations against the raw output lines.
///
/// For each `ChunkConfig`, splits the output at `split_on` boundaries, extracts
/// structured data from each block, and optionally groups by a field.
/// Regexes are compiled once per config, not per chunk.
///
/// Returns a map from `collect_as` names to vectors of structured items.
pub fn process_chunks(configs: &[ChunkConfig], lines: &[&str]) -> HashMap<String, Vec<ChunkItem>> {
    let mut result = HashMap::new();
    for config in configs {
        let Ok(re) = Regex::new(&config.split_on) else {
            eprintln!(
                "[tokf] chunk: invalid split_on regex {:?}, skipping",
                config.split_on
            );
            continue;
        };
        let compiled = CompiledChunkConfig::new(config);
        let raw_chunks = split_at_boundaries(lines, &re, config.include_split_line);
        let mut items: Vec<ChunkItem> = raw_chunks
            .iter()
            .map(|chunk| process_single_chunk(chunk, &compiled))
            .collect();

        normalize_keys(&mut items);

        if let Some(ref group_field) = config.group_by {
            items = group_by_field(&items, group_field);
        }

        result.insert(config.collect_as.clone(), items);
    }
    result
}

/// Split lines into chunks at each match of the split regex.
///
/// Each match starts a new chunk. The first lines before any match are discarded
/// (they belong to no chunk). When `include_header` is true, the matching line
/// is included as the first line of its chunk.
fn split_at_boundaries<'a>(
    lines: &[&'a str],
    split_re: &Regex,
    include_header: bool,
) -> Vec<Vec<&'a str>> {
    let mut chunks: Vec<Vec<&'a str>> = Vec::new();
    let mut current: Option<Vec<&'a str>> = None;

    for &line in lines {
        if split_re.is_match(line) {
            if let Some(chunk) = current.take() {
                chunks.push(chunk);
            }
            let mut new_chunk = Vec::new();
            if include_header {
                new_chunk.push(line);
            }
            current = Some(new_chunk);
        } else if let Some(ref mut chunk) = current {
            chunk.push(line);
        }
        // Lines before first match are discarded
    }

    if let Some(chunk) = current {
        chunks.push(chunk);
    }

    chunks
}

/// Process a single raw chunk into a structured item using pre-compiled regexes.
fn process_single_chunk(chunk_lines: &[&str], compiled: &CompiledChunkConfig<'_>) -> ChunkItem {
    let mut item = ChunkItem::new();
    let config = compiled.config;

    // Extract from header line
    if let Some(ref extract) = config.extract
        && let Some(ref re) = compiled.extract_re
        && let Some(header) = chunk_lines.first()
        && let Some(caps) = re.captures(header)
        && let Some(m) = caps.get(1)
    {
        item.insert(extract.as_name.clone(), m.as_str().to_string());
    }

    // Body extractions (first match per rule wins)
    for (body_ext, re_opt) in config.body_extract.iter().zip(&compiled.body_extract_res) {
        if let Some(re) = re_opt {
            for &line in chunk_lines {
                if let Some(caps) = re.captures(line)
                    && let Some(m) = caps.get(1)
                {
                    item.insert(body_ext.as_name.clone(), m.as_str().to_string());
                    break;
                }
            }
        }
    }

    // Per-chunk aggregation using pre-compiled regexes.
    if !config.aggregate.is_empty() {
        let owned_lines: Vec<String> = chunk_lines.iter().map(|s| (*s).to_string()).collect();
        for (rule, re_opt) in config.aggregate.iter().zip(&compiled.aggregate_res) {
            if let Some(re) = re_opt {
                let agg_result =
                    super::aggregate::aggregate_over_lines_with_regex(&owned_lines, rule, re);
                item.extend(agg_result);
            }
        }
    }

    item
}

/// Ensure all chunk items have the same key set.
///
/// Collects the union of all keys across items, then fills missing keys with
/// empty string. This prevents outer branch-aggregate variables from bleeding
/// through in `each:` templates when a chunk item is missing a field.
fn normalize_keys(items: &mut [ChunkItem]) {
    let all_keys: HashSet<String> = items.iter().flat_map(|item| item.keys().cloned()).collect();
    for item in items.iter_mut() {
        for key in &all_keys {
            item.entry(key.clone()).or_insert_with(String::new);
        }
    }
}

/// Group chunk items by a field, merging numeric fields by summing.
///
/// Non-numeric fields keep the value from the first item in each group.
fn group_by_field(items: &[ChunkItem], field: &str) -> Vec<ChunkItem> {
    let mut groups: Vec<(String, ChunkItem)> = Vec::new();

    for item in items {
        let key = item.get(field).cloned().unwrap_or_default();
        if let Some((_, existing)) = groups.iter_mut().find(|(k, _)| k == &key) {
            // Merge: sum numeric fields, keep first non-numeric
            for (k, v) in item {
                if let Some(existing_val) = existing.get(k) {
                    if let (Ok(a), Ok(b)) = (existing_val.parse::<i64>(), v.parse::<i64>()) {
                        existing.insert(k.clone(), (a + b).to_string());
                    }
                    // Non-numeric: keep existing (first wins)
                } else {
                    existing.insert(k.clone(), v.clone());
                }
            }
        } else {
            groups.push((key, item.clone()));
        }
    }

    groups.into_iter().map(|(_, item)| item).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tokf_common::config::types::{ChunkAggregateRule, ChunkBodyExtract, ChunkExtract};

    fn basic_config() -> ChunkConfig {
        ChunkConfig {
            split_on: r"^\s*Running ".to_string(),
            include_split_line: true,
            collect_as: "suites".to_string(),
            extract: Some(ChunkExtract {
                pattern: r"deps/([\w_-]+)-".to_string(),
                as_name: "crate".to_string(),
            }),
            body_extract: vec![],
            aggregate: vec![ChunkAggregateRule {
                pattern: r"(\d+) passed".to_string(),
                sum: Some("passed".to_string()),
                count_as: None,
            }],
            group_by: None,
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
        let suites = &result["suites"];
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
        let suites = &result["suites"];
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
        let suites = &result["suites"];
        assert_eq!(suites.len(), 1);
        assert!(!suites[0].contains_key("crate")); // header was excluded
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
        }];
        let result = process_chunks(&[config], &lines);
        let suites = &result["suites"];
        assert_eq!(suites[0]["total_tests"], "42");
    }

    #[test]
    fn empty_input() {
        let config = basic_config();
        let result = process_chunks(&[config], &[]);
        assert!(result["suites"].is_empty());
    }

    #[test]
    fn no_matches() {
        let lines = vec!["no matching lines here", "just noise"];
        let config = basic_config();
        let result = process_chunks(&[config], &lines);
        assert!(result["suites"].is_empty());
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
        let suites = &result["suites"];
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
        };
        let result = process_chunks(&[config], &lines);
        let suites = &result["suites"];
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
        normalize_keys(&mut items);
        assert_eq!(items[0]["passed"], "10");
        assert_eq!(items[1]["passed"], ""); // filled with empty string
        assert_eq!(items[0]["crate"], "alpha");
        assert_eq!(items[1]["crate"], "beta");
    }

    #[test]
    fn normalize_keys_no_items() {
        let mut items: Vec<ChunkItem> = vec![];
        normalize_keys(&mut items); // should not panic
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
        normalize_keys(&mut items);
        assert_eq!(items[0]["a"], "1");
        assert_eq!(items[0]["b"], "2");
        assert_eq!(items[1]["a"], "3");
        assert_eq!(items[1]["b"], "4");
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
        let suites = &result["suites"];
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
        let suites = &result["suites"];
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
        let suites = &result["suites"];
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
        });
        let result = process_chunks(&[config], &lines);
        let suites = &result["suites"];
        assert_eq!(suites.len(), 1);
        // Extract failed gracefully — no "name" key
        assert!(!suites[0].contains_key("name"));
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
        }];
        let result = process_chunks(&[config], &lines);
        let suites = &result["suites"];
        assert_eq!(suites.len(), 1);
        assert!(!suites[0].contains_key("total")); // skipped gracefully
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
            }),
            body_extract: vec![],
            aggregate: vec![ChunkAggregateRule {
                pattern: r"(\d+) items".to_string(),
                sum: Some("count".to_string()),
                count_as: None,
            }],
            group_by: None,
        };
        let config2 = ChunkConfig {
            split_on: "^=== ".to_string(),
            include_split_line: true,
            collect_as: "equals".to_string(),
            extract: Some(ChunkExtract {
                pattern: r"=== (\w+)".to_string(),
                as_name: "label".to_string(),
            }),
            body_extract: vec![],
            aggregate: vec![],
            group_by: None,
        };
        let result = process_chunks(&[config1, config2], &lines);
        assert_eq!(result["dashes"].len(), 2);
        assert_eq!(result["dashes"][0]["name"], "alpha");
        assert_eq!(result["dashes"][0]["count"], "10");
        assert_eq!(result["equals"].len(), 2);
        assert_eq!(result["equals"][0]["label"], "X");
        assert_eq!(result["equals"][1]["label"], "Y");
    }
}
