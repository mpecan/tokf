use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};

use serde_json::Value;

use super::types::ExtractedCommand;

/// Parse a Claude Code session JSONL stream, extracting Bash `tool_use` commands
/// and pairing them with their `tool_result` outputs.
///
/// Two-pass approach within a single scan:
/// 1. Collect `tool_use` entries (name == `"Bash"`) with their command strings.
/// 2. Match `tool_result` entries by `tool_use_id` to capture output byte sizes.
pub fn parse_session<R: Read>(reader: R) -> Vec<ExtractedCommand> {
    let buf = BufReader::new(reader);
    let mut commands: HashMap<String, ExtractedCommand> = HashMap::new();
    let mut pending_results: HashMap<String, usize> = HashMap::new();

    for line in buf.lines() {
        let Ok(line) = line else { continue };
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        extract_from_value(&val, &mut commands, &mut pending_results);
    }

    // Apply any pending results that arrived before their tool_use
    for (id, bytes) in &pending_results {
        if let Some(cmd) = commands.get_mut(id) {
            cmd.output_bytes = *bytes;
        }
    }

    let mut result: Vec<ExtractedCommand> = commands.into_values().collect();
    result.sort_by(|a, b| a.tool_use_id.cmp(&b.tool_use_id));
    result
}

fn extract_from_value(
    val: &Value,
    commands: &mut HashMap<String, ExtractedCommand>,
    pending_results: &mut HashMap<String, usize>,
) {
    let msg_type = val.get("type").and_then(Value::as_str).unwrap_or("");

    match msg_type {
        "assistant" | "user" => {
            let content = val.pointer("/message/content").and_then(Value::as_array);
            if let Some(blocks) = content {
                process_content_blocks(blocks, commands, pending_results);
            }
        }
        "progress" => {
            // Sub-agent messages nest deeper
            let content = val
                .pointer("/data/message/message/content")
                .and_then(Value::as_array);
            if let Some(blocks) = content {
                process_content_blocks(blocks, commands, pending_results);
            }
        }
        _ => {}
    }
}

fn process_content_blocks(
    blocks: &[Value],
    commands: &mut HashMap<String, ExtractedCommand>,
    pending_results: &mut HashMap<String, usize>,
) {
    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("");

        if block_type == "tool_use" && block.get("name").and_then(Value::as_str) == Some("Bash") {
            if let Some(cmd) = extract_tool_use(block) {
                let id = cmd.tool_use_id.clone();
                let output_bytes = pending_results.remove(&id).unwrap_or(0);
                commands.insert(
                    id,
                    ExtractedCommand {
                        output_bytes,
                        ..cmd
                    },
                );
            }
        } else if block_type == "tool_result"
            && let Some((id, bytes)) = extract_tool_result(block)
        {
            if let Some(cmd) = commands.get_mut(&id) {
                cmd.output_bytes = bytes;
            } else {
                pending_results.insert(id, bytes);
            }
        }
    }
}

fn extract_tool_use(block: &Value) -> Option<ExtractedCommand> {
    let id = block.get("id").and_then(Value::as_str)?;
    let command = block.pointer("/input/command").and_then(Value::as_str)?;
    Some(ExtractedCommand {
        tool_use_id: id.to_string(),
        command: command.to_string(),
        output_bytes: 0,
    })
}

fn extract_tool_result(block: &Value) -> Option<(String, usize)> {
    let id = block.get("tool_use_id").and_then(Value::as_str)?;
    let content = block.get("content")?;

    let bytes = content.as_str().map_or_else(
        || {
            content.as_array().map_or(0, |arr| {
                arr.iter()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .map(str::len)
                    .sum()
            })
        },
        str::len,
    );

    Some((id.to_string(), bytes))
}
