use std::collections::HashMap;

use regex::Regex;

use super::chunk::{ChunkData, ChunkItem};
use super::section::SectionMap;

/// Chunks map: `collect_as` name → chunk data (flat or tree).
pub type ChunkMap = HashMap<String, ChunkData>;

/// Maximum recursion depth to prevent infinite loops.
const MAX_DEPTH: usize = 3;

/// Bundles the three lookup sources for template variable resolution.
struct TemplateContext<'a> {
    vars: &'a HashMap<String, String>,
    sections: &'a SectionMap,
    chunks: &'a ChunkMap,
}

/// Render a template string, resolving `{var}`, `{var.count}`, and pipe chains.
///
/// Variables are looked up first in `vars` (string values), then in `sections`
/// (collection values), then in `chunks` (structured collection values).
/// Pipe operations transform the resolved value.
pub fn render_template(
    template: &str,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
    chunks: &ChunkMap,
) -> String {
    let ctx = TemplateContext {
        vars,
        sections,
        chunks,
    };
    render_template_inner(template, &ctx, 0)
}

fn render_template_inner(template: &str, ctx: &TemplateContext<'_>, depth: usize) -> String {
    if depth >= MAX_DEPTH {
        return template.to_string();
    }

    let expressions = find_expressions(template);
    if expressions.is_empty() {
        return template.to_string();
    }

    let mut result = template.to_string();

    // Process right-to-left to preserve offsets
    for (start, end) in expressions.into_iter().rev() {
        let inner = &template[start + 1..end - 1]; // strip { }
        let replacement = evaluate_expression(inner, ctx, depth);
        result.replace_range(start..end, &replacement);
    }

    result
}

/// Find top-level `{...}` expression spans, handling nested braces and quotes.
/// Returns (start, end) byte offsets where end is exclusive (points after `}`).
fn find_expressions(template: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        if bytes[i] == b'{' {
            if let Some(end) = find_matching_close(bytes, i) {
                result.push((i, end + 1));
                i = end + 1;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    result
}

/// Find the matching `}` for an opening `{` at `start`, respecting nesting and quotes.
fn find_matching_close(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_quote = false;
    let mut i = start;

    while i < bytes.len() {
        let ch = bytes[i];

        if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_quote = !in_quote;
        } else if !in_quote {
            if ch == b'{' {
                depth += 1;
            } else if ch == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }

        i += 1;
    }

    None
}

/// Resolved value — either a single string, a flat collection, structured items,
/// or a tree (grouped items with children).
enum Value {
    Str(String),
    Collection(Vec<String>),
    StructuredCollection(Vec<ChunkItem>),
    TreeCollection {
        groups: Vec<ChunkItem>,
        children_key: String,
        children: Vec<Vec<ChunkItem>>,
    },
}

/// Evaluate a single expression: resolve variable, apply pipe chain.
fn evaluate_expression(expr: &str, ctx: &TemplateContext<'_>, depth: usize) -> String {
    let parts = split_pipes(expr);
    let var_part = parts[0].trim();
    let pipes = &parts[1..];

    // Resolve the variable
    let mut value = resolve_variable(var_part, ctx);

    // Apply each pipe
    for pipe_str in pipes {
        value = apply_pipe(pipe_str.trim(), value, ctx, depth);
    }

    // Convert final value to string
    match value {
        Value::Str(s) => s,
        Value::Collection(items) => items.join(", "),
        Value::StructuredCollection(items) => items
            .iter()
            .map(format_chunk_item)
            .collect::<Vec<_>>()
            .join(", "),
        Value::TreeCollection { groups, .. } => groups
            .iter()
            .map(format_chunk_item)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Split an expression on top-level `|` (not inside quotes or nested braces).
fn split_pipes(expr: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let bytes = expr.as_bytes();
    let mut last = 0;
    let mut brace_depth = 0;
    let mut in_quote = false;

    for (i, &ch) in bytes.iter().enumerate() {
        if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_quote = !in_quote;
        } else if !in_quote {
            if ch == b'{' {
                brace_depth += 1;
            } else if ch == b'}' {
                brace_depth -= 1;
            } else if ch == b'|' && brace_depth == 0 {
                result.push(&expr[last..i]);
                last = i + 1;
            }
        }
    }

    result.push(&expr[last..]);
    result
}

/// Resolve a variable name to a Value.
fn resolve_variable(name: &str, ctx: &TemplateContext<'_>) -> Value {
    // Check for property access (e.g., "var.count")
    if let Some((base, prop)) = name.split_once('.') {
        let base = base.trim();
        let prop = prop.trim();

        if prop == "count" {
            if let Some(section_data) = ctx.sections.get(base) {
                return Value::Str(section_data.count().to_string());
            }
            if let Some(chunk_data) = ctx.chunks.get(base) {
                return Value::Str(chunk_data.len().to_string());
            }
        }

        // Unknown property → empty
        return Value::Str(String::new());
    }

    // Plain variable: check vars first, then sections, then chunks
    if let Some(val) = ctx.vars.get(name) {
        return Value::Str(val.clone());
    }

    if let Some(section_data) = ctx.sections.get(name) {
        return Value::Collection(section_data.items().to_vec());
    }

    if let Some(chunk_data) = ctx.chunks.get(name) {
        return match chunk_data {
            ChunkData::Flat(items) => Value::StructuredCollection(items.clone()),
            ChunkData::Tree {
                groups,
                children_key,
                children,
            } => Value::TreeCollection {
                groups: groups.clone(),
                children_key: children_key.clone(),
                children: children.clone(),
            },
        };
    }

    Value::Str(String::new())
}

/// Apply a single pipe operation to a value.
fn apply_pipe(pipe: &str, value: Value, ctx: &TemplateContext<'_>, depth: usize) -> Value {
    if let Some(arg) = pipe.strip_prefix("join:") {
        apply_join(arg.trim(), value)
    } else if let Some(arg) = pipe.strip_prefix("each:") {
        apply_each(arg.trim(), value, ctx, depth)
    } else if let Some(arg) = pipe.strip_prefix("truncate:") {
        apply_truncate(arg.trim(), value)
    } else if pipe == "lines" {
        apply_lines(value)
    } else if let Some(arg) = pipe
        .strip_prefix("keep:")
        .or_else(|| pipe.strip_prefix("where:"))
    {
        apply_keep_pipe(arg.trim(), value)
    } else {
        value // unknown pipe → passthrough
    }
}

/// `| join: "separator"` — join a collection into a string.
fn apply_join(arg: &str, value: Value) -> Value {
    let sep = parse_string_arg(arg);

    match value {
        Value::Collection(items) => Value::Str(items.join(&sep)),
        Value::StructuredCollection(items) => {
            let strs: Vec<String> = items.iter().map(format_chunk_item).collect();
            Value::Str(strs.join(&sep))
        }
        Value::TreeCollection { groups, .. } => {
            let strs: Vec<String> = groups.iter().map(format_chunk_item).collect();
            Value::Str(strs.join(&sep))
        }
        Value::Str(s) => Value::Str(s), // already a string
    }
}

/// Render one iteration of an `each` template with index/value vars injected.
#[allow(clippy::too_many_arguments)]
fn render_each_item(
    tmpl: &str,
    ctx: &TemplateContext<'_>,
    depth: usize,
    index: usize,
    value: String,
    extra: &ChunkItem,
) -> String {
    let mut local_vars = ctx.vars.clone();
    local_vars.insert("index".to_string(), (index + 1).to_string());
    local_vars.insert("value".to_string(), value);
    for (k, v) in extra {
        local_vars.insert(k.clone(), v.clone());
    }
    let local_ctx = TemplateContext {
        vars: &local_vars,
        sections: ctx.sections,
        chunks: ctx.chunks,
    };
    render_template_inner(tmpl, &local_ctx, depth + 1)
}

/// `| each: "template"` — map each item through a sub-template.
///
/// For flat collections, injects `{index}` and `{value}`.
/// For structured collections, also injects each item's named fields.
fn apply_each(arg: &str, value: Value, ctx: &TemplateContext<'_>, depth: usize) -> Value {
    let tmpl = parse_string_arg(arg);
    let empty = HashMap::new();

    match value {
        Value::Collection(items) => {
            let mapped = items
                .iter()
                .enumerate()
                .map(|(i, item)| render_each_item(&tmpl, ctx, depth, i, item.clone(), &empty))
                .collect();
            Value::Collection(mapped)
        }
        Value::StructuredCollection(items) => {
            let mapped = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    render_each_item(&tmpl, ctx, depth, i, format_chunk_item(item), item)
                })
                .collect();
            Value::Collection(mapped)
        }
        Value::TreeCollection {
            groups,
            children_key,
            children,
        } => {
            let mapped = groups
                .iter()
                .zip(&children)
                .enumerate()
                .map(|(i, (item, child_items))| {
                    let mut local_chunks = ctx.chunks.clone();
                    local_chunks.insert(children_key.clone(), ChunkData::Flat(child_items.clone()));
                    let child_ctx = TemplateContext {
                        vars: ctx.vars,
                        sections: ctx.sections,
                        chunks: &local_chunks,
                    };
                    render_each_item(&tmpl, &child_ctx, depth, i, format_chunk_item(item), item)
                })
                .collect();
            Value::Collection(mapped)
        }
        Value::Str(s) => {
            if s.is_empty() {
                return Value::Collection(Vec::new());
            }
            Value::Collection(vec![render_each_item(&tmpl, ctx, depth, 0, s, &empty)])
        }
    }
}

/// Format a chunk item as a human-readable string (for `{value}` in `each`).
fn format_chunk_item(item: &ChunkItem) -> String {
    let mut parts: Vec<String> = item.iter().map(|(k, v)| format!("{k}={v}")).collect();
    parts.sort();
    parts.join(", ")
}

/// `| truncate: N` — truncate a string to N characters.
fn apply_truncate(arg: &str, value: Value) -> Value {
    let n: usize = match arg.trim().parse() {
        Ok(n) => n,
        Err(_) => return value,
    };

    match value {
        Value::Str(s) => {
            let char_count = s.chars().count();
            if char_count <= n {
                Value::Str(s)
            } else {
                let truncated: String = s.chars().take(n).collect();
                Value::Str(format!("{truncated}..."))
            }
        }
        Value::Collection(items) => {
            // Truncate each item
            let truncated: Vec<String> = items
                .into_iter()
                .map(|s| {
                    let char_count = s.chars().count();
                    if char_count <= n {
                        s
                    } else {
                        let t: String = s.chars().take(n).collect();
                        format!("{t}...")
                    }
                })
                .collect();
            Value::Collection(truncated)
        }
        sc @ Value::StructuredCollection(_) => sc, // passthrough
        tc @ Value::TreeCollection { .. } => tc,   // passthrough
    }
}

/// `| lines` — split a string value into a collection on newline boundaries.
///
/// Collections pass through unchanged.
fn apply_lines(value: Value) -> Value {
    match value {
        Value::Str(s) => Value::Collection(s.lines().map(str::to_string).collect()),
        c @ Value::Collection(_) => c,
        sc @ Value::StructuredCollection(_) => sc,
        tc @ Value::TreeCollection { .. } => tc,
    }
}

/// `| keep: "re"` / `| where: "re"` — retain only collection items matching the regex.
///
/// For structured collections, filters by the `format_chunk_item` representation.
/// Strings and invalid patterns pass through unchanged.
fn apply_keep_pipe(arg: &str, value: Value) -> Value {
    let pattern = parse_string_arg(arg);
    let Ok(re) = Regex::new(&pattern) else {
        return value;
    };
    match value {
        Value::Collection(items) => {
            Value::Collection(items.into_iter().filter(|l| re.is_match(l)).collect())
        }
        Value::StructuredCollection(items) => Value::StructuredCollection(
            items
                .into_iter()
                .filter(|item| re.is_match(&format_chunk_item(item)))
                .collect(),
        ),
        Value::TreeCollection {
            groups,
            children_key,
            children,
        } => {
            let (filtered_groups, filtered_children): (Vec<_>, Vec<_>) = groups
                .into_iter()
                .zip(children)
                .filter(|(item, _)| re.is_match(&format_chunk_item(item)))
                .unzip();
            Value::TreeCollection {
                groups: filtered_groups,
                children_key,
                children: filtered_children,
            }
        }
        s @ Value::Str(_) => s,
    }
}

/// Parse a quoted or unquoted string argument, unescaping `\n`, `\t`, `\\`.
fn parse_string_arg(arg: &str) -> String {
    let trimmed = arg.trim();
    let inner = if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    unescape(inner)
}

/// Unescape `\n` → newline, `\t` → tab, `\"` → quote, `\\` → backslash.
fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('"') => result.push('"'),
                Some('\\') | None => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
