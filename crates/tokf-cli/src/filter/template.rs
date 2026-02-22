use std::collections::HashMap;

use regex::Regex;

use super::section::SectionMap;

/// Maximum recursion depth to prevent infinite loops.
const MAX_DEPTH: usize = 3;

/// Render a template string, resolving `{var}`, `{var.count}`, and pipe chains.
///
/// Variables are looked up first in `vars` (string values), then in `sections`
/// (collection values). Pipe operations transform the resolved value.
pub fn render_template(
    template: &str,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
) -> String {
    render_template_inner(template, vars, sections, 0)
}

fn render_template_inner(
    template: &str,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
    depth: usize,
) -> String {
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
        let replacement = evaluate_expression(inner, vars, sections, depth);
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

/// Resolved value — either a single string or a collection.
enum Value {
    Str(String),
    Collection(Vec<String>),
}

/// Evaluate a single expression: resolve variable, apply pipe chain.
fn evaluate_expression(
    expr: &str,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
    depth: usize,
) -> String {
    let parts = split_pipes(expr);
    let var_part = parts[0].trim();
    let pipes = &parts[1..];

    // Resolve the variable
    let mut value = resolve_variable(var_part, vars, sections);

    // Apply each pipe
    for pipe_str in pipes {
        value = apply_pipe(pipe_str.trim(), value, vars, sections, depth);
    }

    // Convert final value to string
    match value {
        Value::Str(s) => s,
        Value::Collection(items) => items.join(", "),
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
fn resolve_variable(name: &str, vars: &HashMap<String, String>, sections: &SectionMap) -> Value {
    // Check for property access (e.g., "var.count")
    if let Some((base, prop)) = name.split_once('.') {
        let base = base.trim();
        let prop = prop.trim();

        if prop == "count"
            && let Some(section_data) = sections.get(base)
        {
            return Value::Str(section_data.count().to_string());
        }

        // Unknown property → empty
        return Value::Str(String::new());
    }

    // Plain variable: check vars first, then sections
    if let Some(val) = vars.get(name) {
        return Value::Str(val.clone());
    }

    if let Some(section_data) = sections.get(name) {
        return Value::Collection(section_data.items().to_vec());
    }

    Value::Str(String::new())
}

/// Apply a single pipe operation to a value.
fn apply_pipe(
    pipe: &str,
    value: Value,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
    depth: usize,
) -> Value {
    if let Some(arg) = pipe.strip_prefix("join:") {
        apply_join(arg.trim(), value)
    } else if let Some(arg) = pipe.strip_prefix("each:") {
        apply_each(arg.trim(), value, vars, sections, depth)
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
        Value::Str(s) => Value::Str(s), // already a string
    }
}

/// `| each: "template"` — map each item through a sub-template.
fn apply_each(
    arg: &str,
    value: Value,
    vars: &HashMap<String, String>,
    sections: &SectionMap,
    depth: usize,
) -> Value {
    let tmpl = parse_string_arg(arg);

    let items = match value {
        Value::Collection(items) => items,
        Value::Str(s) => {
            if s.is_empty() {
                return Value::Collection(Vec::new());
            }
            vec![s]
        }
    };

    let mapped: Vec<String> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut local_vars = vars.clone();
            local_vars.insert("index".to_string(), (i + 1).to_string());
            local_vars.insert("value".to_string(), item.clone());
            render_template_inner(&tmpl, &local_vars, sections, depth + 1)
        })
        .collect();

    Value::Collection(mapped)
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
    }
}

/// `| lines` — split a string value into a collection on newline boundaries.
///
/// Collections pass through unchanged.
fn apply_lines(value: Value) -> Value {
    match value {
        Value::Str(s) => Value::Collection(s.lines().map(str::to_string).collect()),
        c @ Value::Collection(_) => c,
    }
}

/// `| keep: "re"` / `| where: "re"` — retain only collection items matching the regex.
///
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
mod tests {
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
}
