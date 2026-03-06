use std::collections::HashMap;

use serde_json::Value;
use serde_json_path::JsonPath;

use tokf_common::config::types::{JsonConfig, JsonExtractRule};

use super::chunk::{ChunkData, ChunkItem};
use super::template::ChunkMap;

/// Extract variables and chunks from JSON output using `JSONPath` queries.
///
/// For each extraction rule:
/// - A single scalar result → `vars[as_name] = string_value`
/// - Multiple results without `fields` → `ChunkData::Flat` with a `value` key per item
/// - Multiple results with `fields` → `ChunkData::Flat` with named field keys per item
///
/// Array results also generate a `{as_name}_count` variable.
///
/// Returns `(vars, chunks)` — both empty if the input is not valid JSON.
pub fn extract_json(stdout: &str, config: &JsonConfig) -> (HashMap<String, String>, ChunkMap) {
    let mut vars = HashMap::new();
    let mut chunks = ChunkMap::new();

    let Ok(root) = serde_json::from_str::<Value>(stdout) else {
        return (vars, chunks);
    };

    for rule in &config.extract {
        let Ok(path) = JsonPath::parse(&rule.path) else {
            continue;
        };

        let node_list = path.query(&root);
        let nodes: Vec<&Value> = node_list.all();

        if nodes.is_empty() {
            // Still emit _count = "0" so templates can show "Items (0):"
            if !rule.fields.is_empty() {
                vars.insert(format!("{}_count", rule.as_name), "0".to_string());
            }
            continue;
        }

        if nodes.len() == 1 && rule.fields.is_empty() && !nodes[0].is_array() {
            // Single scalar → template variable
            vars.insert(rule.as_name.clone(), json_value_to_string(nodes[0]));
        } else {
            process_multi_result(rule, &nodes, &mut vars, &mut chunks);
        }
    }

    (vars, chunks)
}

/// Process a multi-value `JSONPath` result into vars + chunks.
fn process_multi_result(
    rule: &JsonExtractRule,
    nodes: &[&Value],
    vars: &mut HashMap<String, String>,
    chunks: &mut ChunkMap,
) {
    let items: Vec<ChunkItem> = if rule.fields.is_empty() {
        // No field extraction — use scalar value or flatten top-level object fields.
        nodes
            .iter()
            .map(|v| {
                if v.is_object() {
                    flatten_object_scalars(v)
                } else {
                    let mut item = ChunkItem::new();
                    item.insert("value".to_string(), json_value_to_string(v));
                    item
                }
            })
            .collect()
    } else {
        // Extract named fields from each matched object.
        nodes
            .iter()
            .map(|v| extract_fields(v, &rule.fields))
            .collect()
    };

    vars.insert(format!("{}_count", rule.as_name), items.len().to_string());
    chunks.insert(rule.as_name.clone(), ChunkData::Flat(items));
}

/// Extract named sub-fields from a JSON value using dot-paths.
fn extract_fields(
    value: &Value,
    fields: &[tokf_common::config::types::JsonFieldExtract],
) -> ChunkItem {
    let mut item = ChunkItem::new();
    for field in fields {
        let extracted = extract_dot_path(value, &field.field).unwrap_or_default();
        item.insert(field.as_name.clone(), extracted);
    }
    item
}

/// Convert a JSON value to a display string.
///
/// - Strings → unquoted
/// - Numbers/bools → their JSON representation
/// - Null → "null"
/// - Objects/arrays → compact JSON
pub fn json_value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => v.to_string(),
    }
}

/// Follow a dot-separated path within a JSON value to extract a scalar.
///
/// Supports both object keys and numeric array indices:
/// - `extract_dot_path(obj, "metadata.name")` → `obj["metadata"]["name"]`
/// - `extract_dot_path(obj, "containers.0.name")` → `obj["containers"][0]["name"]`
pub fn extract_dot_path(value: &Value, path: &str) -> Option<String> {
    let mut current = value;
    for segment in path.split('.') {
        current = if let Ok(idx) = segment.parse::<usize>() {
            current.get(idx).or_else(|| current.get(segment))?
        } else {
            current.get(segment)?
        };
    }
    Some(json_value_to_string(current))
}

/// Extract all top-level scalar fields from a JSON object into a `ChunkItem`.
///
/// Nested objects and arrays are serialized as compact JSON strings.
pub fn flatten_object_scalars(value: &Value) -> ChunkItem {
    let mut item = ChunkItem::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            item.insert(k.clone(), json_value_to_string(v));
        }
    }
    item
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
