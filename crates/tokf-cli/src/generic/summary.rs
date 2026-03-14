use regex::Regex;

/// Summary/footer keyword patterns.
const SUMMARY_PATTERNS: &[&str] = &[
    "total", "summary", "result", "passed", "failed", "error", "warning", "complete", "finished",
    "done", "elapsed", "duration", "built in",
];

/// Precompiled statistics patterns.
/// Each entry: (regex, label, `capture_unit`) — if `capture_unit` is true, the
/// matched unit suffix (s/ms/seconds) is appended to the value.
#[allow(clippy::unwrap_used)]
fn stat_patterns() -> &'static [(Regex, &'static str, bool)] {
    use std::sync::LazyLock;
    static PATTERNS: LazyLock<Vec<(Regex, &'static str, bool)>> = LazyLock::new(|| {
        vec![
            (
                Regex::new(r"(\d+)\s+(?:tests?\s+)?passed").unwrap(),
                "passed",
                false,
            ),
            (
                Regex::new(r"(\d+)\s+(?:tests?\s+)?failed").unwrap(),
                "failed",
                false,
            ),
            (Regex::new(r"(\d+)\s+errors?").unwrap(), "errors", false),
            (Regex::new(r"(\d+)\s+warnings?").unwrap(), "warnings", false),
            (
                Regex::new(r"(\d+)\s+(?:files?|modules?)").unwrap(),
                "files",
                false,
            ),
            (
                Regex::new(r"(?:in\s+)?(\d+\.?\d*)\s*(s|seconds?|ms|minutes?)").unwrap(),
                "time",
                true,
            ),
        ]
    });
    &PATTERNS
}

/// Produce a heuristic summary of command output.
///
/// Three phases:
/// 1. Structure identification: header, footer/summary, repetitive middle
/// 2. Budget allocation: header (up to 5), footer (up to 10), sampled middle
/// 3. Statistics extraction: scan for counts and timing
pub fn summarize(text: &str, exit_code: i32, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();

    // Very short output: pass through unchanged
    if lines.len() < 10 {
        return text.to_string();
    }

    let max_lines = max_lines.max(10); // minimum sensible budget

    // Phase 1: Identify footer/summary region
    let footer_start = find_footer_start(&lines);
    let header_end = 5.min(footer_start).min(lines.len());

    // Phase 2: Budget allocation
    let header_budget = header_end.min(5);
    let footer_budget = (lines.len() - footer_start).min(10).min(max_lines / 2);
    let middle_budget = max_lines.saturating_sub(header_budget + footer_budget + 2);

    let mut result = Vec::new();

    // Header
    for line in &lines[..header_budget] {
        result.push((*line).to_string());
    }

    // Middle (sampled)
    append_middle(
        &lines[header_budget..footer_start],
        middle_budget,
        &mut result,
    );

    // Footer (capped to budget)
    let footer_end = (footer_start + footer_budget).min(lines.len());
    for line in &lines[footer_start..footer_end] {
        result.push((*line).to_string());
    }

    // Phase 3: Statistics
    let stats = extract_stats(text);
    result.push(String::new());
    if stats.is_empty() {
        result.push(format!(
            "[tokf summary] {} lines \u{2192} {} lines (exit code {exit_code})",
            lines.len(),
            result.len() + 1,
        ));
    } else {
        result.push(format!("[tokf summary] {stats}"));
    }

    result.join("\n")
}

fn append_middle(middle: &[&str], budget: usize, result: &mut Vec<String>) {
    if middle.is_empty() {
        return;
    }
    if budget == 0 {
        result.push(format!("... ({} lines omitted)", middle.len()));
        return;
    }
    if middle.len() <= budget {
        for line in middle {
            result.push((*line).to_string());
        }
        return;
    }
    result.push("...".to_string());
    let unique = count_unique_shapes(middle);
    if unique < middle.len() / 2 {
        result.push(format!(
            "[{} lines, {unique} unique patterns \u{2014} showing samples]",
            middle.len()
        ));
        let samples = sample_lines(middle, budget.min(3));
        for s in samples {
            result.push((*s).to_string());
        }
    } else {
        let samples = sample_lines(middle, budget);
        for s in samples {
            result.push((*s).to_string());
        }
    }
    result.push("...".to_string());
}

/// Find the start of the footer/summary region.
fn find_footer_start(lines: &[&str]) -> usize {
    let search_start = lines.len().saturating_sub(15);
    for (i, line) in lines.iter().enumerate().skip(search_start) {
        let lower = line.to_lowercase();
        if SUMMARY_PATTERNS.iter().any(|p| lower.contains(p)) {
            return i;
        }
    }
    // No summary found — use last 3 lines as footer
    lines.len().saturating_sub(3)
}

/// Normalize a line to a "shape" for repetitive detection.
/// Numbers → N, paths → PATH, hashes → HASH.
#[allow(clippy::unwrap_used)]
fn normalize_shape(line: &str) -> String {
    use std::sync::LazyLock;
    static NUMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
    static PATHS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[/\\]\S+").unwrap());
    static HASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap());

    let s = NUMS.replace_all(line, "N");
    let s = PATHS.replace_all(&s, "PATH");
    HASHES.replace_all(&s, "HASH").to_string()
}

/// Count unique shapes in a set of lines.
fn count_unique_shapes(lines: &[&str]) -> usize {
    let mut shapes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in lines {
        shapes.insert(normalize_shape(line));
    }
    shapes.len()
}

/// Evenly sample `count` lines from a slice.
fn sample_lines<'a>(lines: &[&'a str], count: usize) -> Vec<&'a str> {
    if count >= lines.len() {
        return lines.to_vec();
    }
    if count == 0 {
        return vec![];
    }
    let total = lines.len();
    (0..count).map(|i| lines[i * total / count]).collect()
}

/// Extract human-readable statistics from the text.
fn extract_stats(text: &str) -> String {
    let mut parts = Vec::new();
    for (re, label, capture_unit) in stat_patterns() {
        if let Some(cap) = re.captures(text)
            && let Some(val) = cap.get(1)
        {
            if *capture_unit {
                let unit = cap.get(2).map_or("", |m| m.as_str());
                parts.push(format!("{label}: {}{unit}", val.as_str()));
            } else {
                parts.push(format!("{label}: {}", val.as_str()));
            }
        }
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_passthrough() {
        let text = "line 1\nline 2\nline 3";
        assert_eq!(summarize(text, 0, 30), text);
    }

    #[test]
    fn summarize_long_output() {
        let mut lines: Vec<String> = (0..100)
            .map(|i| format!("   Compiling crate_{i}"))
            .collect();
        lines.push("Finished in 5.2 seconds".to_string());
        lines.push("0 errors, 2 warnings".to_string());
        let text = lines.join("\n");
        let result = summarize(&text, 0, 20);
        let result_lines: Vec<&str> = result.lines().collect();
        assert!(result_lines.len() <= 25, "got {} lines", result_lines.len());
        assert!(result.contains("Compiling crate_0"));
        assert!(result.contains("0 errors, 2 warnings"));
        assert!(result.contains("[tokf summary]"));
    }

    #[test]
    fn summarize_repetitive() {
        let mut lines: Vec<String> = (0..50)
            .map(|i| format!("  Compiling dep-{i} v0.{i}.0"))
            .collect();
        lines.push("Finished `dev` profile in 12.3s".to_string());
        let text = lines.join("\n");
        let result = summarize(&text, 0, 15);
        assert!(result.contains("unique patterns"));
    }

    #[test]
    fn normalize_shape_works() {
        assert_eq!(
            normalize_shape("  Compiling dep-42 v0.12.0"),
            "  Compiling dep-N vN.N.N"
        );
        assert_eq!(
            normalize_shape("file /src/foo/bar.rs changed"),
            "file PATH changed"
        );
    }

    #[test]
    fn extract_stats_finds_counts() {
        let text = "test result: ok. 15 passed; 2 failed; 0 ignored; finished in 3.5s";
        let stats = extract_stats(text);
        assert!(stats.contains("passed: 15"), "stats: {stats}");
        assert!(stats.contains("failed: 2"), "stats: {stats}");
        assert!(stats.contains("time: 3.5s"), "stats: {stats}");
    }

    #[test]
    fn sample_lines_even() {
        let lines = vec!["a", "b", "c", "d", "e", "f"];
        let sampled = sample_lines(&lines, 3);
        assert_eq!(sampled.len(), 3);
        assert_eq!(sampled[0], "a");
    }
}
