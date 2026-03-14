use super::patterns::{PatternMatcher, extract_with_context};

/// Error/warning patterns that match across common toolchains.
const ERROR_PATTERNS: &[&str] = &[
    r"(?i)^error[\[:\s]",
    r"(?i)^warning[\[:\s]",
    r"(?i)\berror\b.*:",
    r"(?i)\bwarning\b.*:",
    r"^E\d{4}:", // Rust error codes
    r"(?i)^fatal:",
    r"(?i)^failed:",
    r"Traceback \(most recent",
    r#"^\s+File ""#,   // Python traceback frames
    r"^\w+Error:",     // Python exception types (ValueError:, TypeError:, etc.)
    r"^\w+Exception:", // Java/Python exceptions
    r"^npm ERR!",
    r"^ERR!",
    r"^\s*\^+\s*$",           // Caret lines pointing to errors
    r"^\s*\|\s*\^",           // Rust-style error pointer
    r"^thread '.*' panicked", // Rust panics
    r"FAILED",
];

/// Extract errors and warnings from command output.
///
/// Returns a summary header followed by error lines with context.
pub fn extract_errors(text: &str, exit_code: i32, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();

    // Very short output: pass through unchanged
    if lines.len() < 10 {
        return text.to_string();
    }

    let matcher = PatternMatcher::new(ERROR_PATTERNS, &[]);
    let extracted = extract_with_context(text, &matcher, context);

    if extracted.is_empty() {
        if exit_code == 0 {
            return "[tokf err] no errors detected".to_string();
        }
        // Non-zero exit but no pattern matches — include full stderr
        return format!("[tokf err] exit code {exit_code}, no recognized error patterns\n{text}");
    }

    let error_count = lines
        .iter()
        .filter(|l| matcher.classify(l) == super::patterns::LineKind::Interesting)
        .count();
    let extracted_lines = extracted.lines().count();

    format!(
        "[tokf err] {error_count} error/warning lines extracted \
         ({extracted_lines} lines with context, from {} total)\n{extracted}",
        lines.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_passthrough() {
        let text = "error: bad thing\nline 2";
        assert_eq!(extract_errors(text, 1, 3), text);
    }

    #[test]
    fn no_errors_exit_zero() {
        let text = (0..20)
            .map(|i| format!("ok line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = extract_errors(&text, 0, 3);
        assert_eq!(result, "[tokf err] no errors detected");
    }

    #[test]
    fn no_errors_exit_nonzero() {
        let text = (0..20)
            .map(|i| format!("ok line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = extract_errors(&text, 1, 3);
        assert!(result.starts_with("[tokf err] exit code 1"));
        assert!(result.contains("ok line 0"));
    }

    #[test]
    fn extracts_rust_errors() {
        let mut lines: Vec<String> = (0..30).map(|i| format!("   Compiling crate{i}")).collect();
        lines[15] = "error[E0308]: mismatched types".to_string();
        lines[16] = "  --> src/main.rs:10:5".to_string();
        lines[17] = "   |".to_string();
        lines[18] = "   | expected u32, found &str".to_string();
        let text = lines.join("\n");
        let result = extract_errors(&text, 1, 3);
        assert!(result.starts_with("[tokf err]"));
        assert!(result.contains("error[E0308]"));
        assert!(result.contains("mismatched types"));
    }

    #[test]
    fn extracts_python_traceback() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("output line {i}")).collect();
        lines[10] = "Traceback (most recent call last):".to_string();
        lines[11] = r#"  File "main.py", line 5, in <module>"#.to_string();
        lines[12] = "    x = 1 / 0".to_string();
        lines[13] = "ZeroDivisionError: division by zero".to_string();
        let text = lines.join("\n");
        let result = extract_errors(&text, 1, 3);
        assert!(result.contains("Traceback"));
        assert!(result.contains("ZeroDivisionError"));
    }

    #[test]
    fn extracts_npm_errors() {
        let mut lines: Vec<String> = (0..20).map(|i| format!("npm info {i}")).collect();
        lines[10] = "npm ERR! code ENOENT".to_string();
        lines[11] = "npm ERR! syscall open".to_string();
        let text = lines.join("\n");
        let result = extract_errors(&text, 1, 3);
        assert!(result.contains("npm ERR!"));
    }
}
