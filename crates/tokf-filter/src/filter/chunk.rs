use std::collections::{HashMap, HashSet};

use regex::Regex;

use tokf_common::config::types::ChunkConfig;

/// One processed chunk's extracted fields (key → string value).
pub type ChunkItem = HashMap<String, String>;

/// Processed chunk data — either a flat list or a tree with grouped parents and children.
#[derive(Debug, Clone)]
pub enum ChunkData {
    Flat(Vec<ChunkItem>),
    Tree {
        groups: Vec<ChunkItem>,
        children_key: String,
        children: Vec<Vec<ChunkItem>>,
    },
}

impl ChunkData {
    pub const fn len(&self) -> usize {
        match self {
            Self::Flat(items) => items.len(),
            Self::Tree { groups, .. } => groups.len(),
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

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
/// Returns a map from `collect_as` names to `ChunkData` values.
pub fn process_chunks(configs: &[ChunkConfig], lines: &[&str]) -> HashMap<String, ChunkData> {
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

        apply_carry_forward(config, &mut items);
        normalize_keys(config, &mut items);

        let data = match (&config.group_by, &config.children_as) {
            (Some(group_field), Some(children_key)) => {
                let (groups, children) = group_by_field_with_children(&items, group_field);
                ChunkData::Tree {
                    groups,
                    children_key: children_key.clone(),
                    children,
                }
            }
            (Some(group_field), None) => ChunkData::Flat(group_by_field(&items, group_field)),
            _ => ChunkData::Flat(items),
        };

        result.insert(config.collect_as.clone(), data);
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
/// Collects the union of all keys across items — including configured field
/// names from extract, `body_extract`, and aggregate rules — then fills missing
/// keys with empty string. This prevents outer branch-aggregate variables from
/// bleeding through in `each:` templates when a chunk item is missing a field.
pub(crate) fn normalize_keys(config: &ChunkConfig, items: &mut [ChunkItem]) {
    let mut all_keys: HashSet<String> =
        items.iter().flat_map(|item| item.keys().cloned()).collect();

    // Seed from configured field names so they always exist on every item.
    if let Some(ref extract) = config.extract {
        all_keys.insert(extract.as_name.clone());
    }
    for be in &config.body_extract {
        all_keys.insert(be.as_name.clone());
    }
    for agg in &config.aggregate {
        if let Some(ref sum) = agg.sum {
            all_keys.insert(sum.clone());
        }
        if let Some(ref count_as) = agg.count_as {
            all_keys.insert(count_as.clone());
        }
    }

    for item in items.iter_mut() {
        for key in &all_keys {
            item.entry(key.clone()).or_insert_with(String::new);
        }
    }
}

/// Apply carry-forward logic: for fields with `carry_forward = true`,
/// fill missing or empty values from the most recently extracted value.
fn apply_carry_forward(config: &ChunkConfig, items: &mut [ChunkItem]) {
    // Collect all carry_forward field names
    let mut cf_fields: Vec<String> = Vec::new();
    if let Some(ref extract) = config.extract
        && extract.carry_forward
    {
        cf_fields.push(extract.as_name.clone());
    }
    for be in &config.body_extract {
        if be.carry_forward {
            cf_fields.push(be.as_name.clone());
        }
    }
    if cf_fields.is_empty() {
        return;
    }

    let mut state: HashMap<String, String> = HashMap::new();
    for item in items.iter_mut() {
        for field in &cf_fields {
            let current = item.get(field).cloned().unwrap_or_default();
            if current.is_empty() {
                // Fill from state
                if let Some(prev) = state.get(field) {
                    item.insert(field.clone(), prev.clone());
                }
            } else {
                // Update state
                state.insert(field.clone(), current);
            }
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
            merge_into(existing, item);
        } else {
            groups.push((key, item.clone()));
        }
    }

    groups.into_iter().map(|(_, item)| item).collect()
}

/// Group chunk items by a field, preserving children for tree output.
///
/// Returns `(groups, children)` where `children[i]` contains the original
/// items that were merged into `groups[i]`.
fn group_by_field_with_children(
    items: &[ChunkItem],
    field: &str,
) -> (Vec<ChunkItem>, Vec<Vec<ChunkItem>>) {
    let mut groups: Vec<(String, ChunkItem, Vec<ChunkItem>)> = Vec::new();

    for item in items {
        let key = item.get(field).cloned().unwrap_or_default();
        if let Some((_, existing, children)) = groups.iter_mut().find(|(k, _, _)| k == &key) {
            merge_into(existing, item);
            children.push(item.clone());
        } else {
            groups.push((key, item.clone(), vec![item.clone()]));
        }
    }

    let (merged, children): (Vec<_>, Vec<_>) = groups
        .into_iter()
        .map(|(_, item, children)| (item, children))
        .unzip();
    (merged, children)
}

/// Merge `source` into `target`: sum numeric fields, keep first non-numeric.
///
/// Empty existing values are treated as 0 when the incoming value is numeric,
/// so grouping reliably sums fields regardless of encounter order.
fn merge_into(target: &mut ChunkItem, source: &ChunkItem) {
    for (k, v) in source {
        if let Some(existing_val) = target.get(k) {
            let existing_trimmed = existing_val.trim();
            if existing_trimmed.is_empty() {
                if let Ok(b) = v.parse::<i64>() {
                    target.insert(k.clone(), b.to_string());
                }
            } else if let (Ok(a), Ok(b)) = (existing_val.parse::<i64>(), v.parse::<i64>()) {
                target.insert(k.clone(), (a + b).to_string());
            }
            // Non-numeric and non-empty existing value: keep existing (first wins)
        } else {
            target.insert(k.clone(), v.clone());
        }
    }
}
