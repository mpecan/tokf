use super::patterns::{LineKind, PatternMatcher, extract_with_context};

/// Patterns indicating test failures.
const FAILURE_PATTERNS: &[&str] = &[
    r"(?i)\bFAIL(ED|URE|S)?\b",
    r"(?i)\bfailed\b",
    r"(?i)^not ok\b",
    r"(?i)assertion.*failed",
    r"(?i)^FAIL\s",
    r"(?i)panicked at",
    r"thread '.*' panicked",
    r"--- FAIL:",         // Go test
    r"^\s+✕",             // Jest ✕
    r"^\s+✗",             // Test failure marker
    r"^\s+×",             // Another failure marker
    r"^\s*Expected\b",    // Assertion mismatch
    r"^\s*Received\b",    // Assertion mismatch (Jest)
    r"^\s*-\s+Expected",  // Diff output
    r"^\s*\+\s+Received", // Diff output
    r"^error\[",          // Rust compile errors in test
    r"left.*right",       // Rust assert_eq! output
];

/// Summary line patterns (always kept).
const SUMMARY_PATTERNS: &[&str] = &[
    r"(?i)^\s*test result:",
    r"(?i)^\s*tests?:\s+\d+",
    r"(?i)^\s*\d+\s+(passed|failed|skipped|pending)",
    r"(?i)^(ok|FAIL)\s+\S+\s+\d+\.\d+s", // Go test summary
    r"(?i)test suites?:",                // Jest
    r"(?i)^Tests:\s+",                   // Jest summary
    r"(?i)^Ran \d+ tests?",              // Python unittest
    r"(?i)^=+\s+(FAILURES|ERRORS|short test summary)", // pytest
    r"(?i)^\d+ examples?, \d+ failures?", // RSpec
];

/// Extract test failure information from command output.
///
/// Highlights failure lines with context; always includes summary lines.
pub fn extract_test_failures(text: &str, exit_code: i32, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();

    // Very short output: pass through unchanged
    if lines.len() < 10 {
        return text.to_string();
    }

    let matcher = PatternMatcher::new(FAILURE_PATTERNS, SUMMARY_PATTERNS);
    let extracted = extract_with_context(text, &matcher, context);

    if extracted.is_empty() {
        if exit_code == 0 {
            return "[tokf test] all tests passed".to_string();
        }
        return format!("[tokf test] exit code {exit_code}, no recognized test patterns\n{text}");
    }

    // Check if all interesting lines are actually just summary (i.e., no real failures)
    let has_failures = lines
        .iter()
        .any(|l| matcher.classify(l) == LineKind::Interesting);

    if !has_failures && exit_code == 0 {
        let deduped = dedup_summary_lines(&extracted);
        return format!("[tokf test] all tests passed\n{deduped}");
    }

    let failure_count = lines
        .iter()
        .filter(|l| matcher.classify(l) == LineKind::Interesting)
        .count();
    let extracted_lines = extracted.lines().count();

    format!(
        "[tokf test] {failure_count} failure lines extracted \
         ({extracted_lines} lines with context, from {} total)\n{extracted}",
        lines.len()
    )
}

/// Collapse repetitive summary lines (e.g. 33 "test result: ok. 0 passed; 0 failed; ..."
/// lines from multi-binary cargo test) into a count + unique examples.
fn dedup_summary_lines(text: &str) -> String {
    use std::collections::HashMap;

    let lines: Vec<&str> = text.lines().collect();
    let mut shape_counts: HashMap<String, (usize, &str)> = HashMap::new();

    for line in &lines {
        let shape = normalize_summary_shape(line);
        shape_counts
            .entry(shape)
            .and_modify(|(count, _)| *count += 1)
            .or_insert((1, line));
    }

    // If no shape appears more than twice, return as-is
    let max_repeats = shape_counts.values().map(|(c, _)| *c).max().unwrap_or(0);
    if max_repeats <= 2 {
        return text.to_string();
    }

    let mut result = Vec::new();
    let mut seen_shapes: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in &lines {
        let shape = normalize_summary_shape(line);
        let (count, _) = shape_counts[&shape];
        if count <= 2 {
            result.push((*line).to_string());
        } else if seen_shapes.insert(shape) {
            // First occurrence of a repeated shape: show it with count
            result.push(format!("{line}  (\u{00d7}{count} similar)"));
        }
        // Skip subsequent duplicates
    }

    result.join("\n")
}

/// Normalize a summary line for dedup comparison.
/// Numbers → N so "0 passed; 0 failed; 839 filtered" matches "7 passed; 0 failed; 177 filtered".
#[allow(clippy::unwrap_used)]
fn normalize_summary_shape(line: &str) -> String {
    use std::sync::LazyLock;
    static NUMS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\d+").unwrap());
    NUMS.replace_all(line, "N").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_passthrough() {
        let text = "FAIL test\nok done";
        assert_eq!(extract_test_failures(text, 1, 5), text);
    }

    #[test]
    fn all_pass_exit_zero() {
        let text = (0..20)
            .map(|i| format!("ok line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = extract_test_failures(&text, 0, 5);
        assert_eq!(result, "[tokf test] all tests passed");
    }

    #[test]
    fn no_patterns_exit_nonzero() {
        let text = (0..20)
            .map(|i| format!("ok line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = extract_test_failures(&text, 1, 5);
        assert!(result.starts_with("[tokf test] exit code 1"));
    }

    #[test]
    fn all_pass_deduplicates_summary_lines() {
        // Simulate cargo test with multiple binaries all passing
        let mut lines: Vec<String> = Vec::new();
        for i in 0..20 {
            lines.push(format!(
                "test result: ok. {i} passed; 0 failed; 0 ignored; 0 measured; {} filtered out; finished in 0.0{}s",
                100 - i, i
            ));
        }
        let text = lines.join("\n");
        let result = extract_test_failures(&text, 0, 5);
        assert!(result.starts_with("[tokf test] all tests passed"));
        // Should be collapsed, not 20 lines
        let result_lines: Vec<&str> = result.lines().collect();
        assert!(
            result_lines.len() < 5,
            "expected collapsed output, got {} lines:\n{result}",
            result_lines.len()
        );
        assert!(result.contains("similar"));
    }

    #[test]
    fn extracts_cargo_test_failures() {
        let mut lines: Vec<String> = (0..30).map(|i| format!("test test_{i} ... ok")).collect();
        lines[15] = "test test_bad ... FAILED".to_string();
        lines[16] = String::new();
        lines[17] = "failures:".to_string();
        lines[18] = "---- test_bad stdout ----".to_string();
        lines[19] = "thread 'test_bad' panicked at 'assertion failed'".to_string();
        lines[28] = "test result: FAILED. 29 passed; 1 failed; 0 ignored".to_string();
        let text = lines.join("\n");
        let result = extract_test_failures(&text, 1, 3);
        assert!(result.starts_with("[tokf test]"));
        assert!(result.contains("FAILED"));
        assert!(result.contains("panicked"));
        assert!(result.contains("test result:"));
    }

    #[test]
    fn extracts_go_test_failures() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("=== RUN   Test{i}")).collect();
        lines[10] = "--- FAIL: TestBad (0.00s)".to_string();
        lines[11] = "    expected 1, got 2".to_string();
        lines[18] = "FAIL\tgithub.com/example/pkg\t0.123s".to_string();
        let text = lines.join("\n");
        let result = extract_test_failures(&text, 1, 3);
        assert!(result.contains("--- FAIL:"));
    }

    #[test]
    fn extracts_jest_failures() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("  ✓ test {i}")).collect();
        lines[10] = "  ✕ should handle errors".to_string();
        lines[11] = "    Expected: true".to_string();
        lines[12] = "    Received: false".to_string();
        lines[18] = "Tests: 1 failed, 19 passed, 20 total".to_string();
        let text = lines.join("\n");
        let result = extract_test_failures(&text, 1, 3);
        assert!(result.contains("✕"));
        assert!(result.contains("Expected"));
        assert!(result.contains("Tests:"));
    }
}
