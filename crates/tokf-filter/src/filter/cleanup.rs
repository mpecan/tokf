use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;
use tokf_common::config::types::FilterConfig;

fn ansi_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Covers:
        //   CSI sequences: \x1b[...letter      (colors, cursor movement)
        //   OSC sequences: \x1b]...(BEL|ST)    (hyperlinks, titles, semantic coloring)
        //   Fe sequences:  \x1b[@-_]           (single-char controls, catch-all)
        // OSC must appear before the [@-_] catch-all because ']' (0x5D) is in
        // that range and would otherwise consume \x1b] as a bare Fe escape.
        // SAFETY: pattern is a compile-time constant and always valid.
        #[allow(clippy::expect_used)]
        Regex::new(r"\x1b(?:\[[0-9;]*[a-zA-Z]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-_])")
            .expect("valid ANSI regex")
    })
}

/// Strip ANSI escape sequences from a single string.
pub fn strip_ansi_from(s: &str) -> String {
    ansi_regex().replace_all(s, "").into_owned()
}

/// Per-line cleanup applied before skip/keep filtering.
///
/// - `strip_ansi`: removes ANSI escape sequences from each line
/// - `trim_lines`: trims leading/trailing whitespace from each line
///
/// Returns an owned `Vec<String>` (same pattern as `replace::apply_replace`).
pub fn apply_line_cleanup(config: &FilterConfig, lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut s = (*line).to_string();
            if config.strip_ansi {
                s = ansi_regex().replace_all(&s, "").into_owned();
            }
            if config.trim_lines {
                s = s.trim().to_string();
            }
            s
        })
        .collect()
}

/// Truncate a single line to `max_chars` characters with a trailing `…`.
///
/// Returns `Cow::Borrowed` when no truncation is needed (avoids allocation).
/// Uses a single pass over the string's characters.
fn truncate_line(line: &str, max_chars: usize) -> Cow<'_, str> {
    // Single-pass: walk char_indices looking for the truncation point.
    // The ellipsis counts within the budget: max_chars=10 → 9 content + "…".
    let keep = max_chars.saturating_sub(1); // chars to keep before "…"
    let mut keep_byte_end = 0; // byte offset for the first `keep` chars
    for (i, (byte_idx, ch)) in line.char_indices().enumerate() {
        if i == max_chars {
            // Line exceeds limit → truncate.
            return Cow::Owned(format!("{}\u{2026}", &line[..keep_byte_end]));
        }
        // Track the byte end of the `keep`-th char (0-indexed: chars 0..keep-1).
        if i < keep {
            keep_byte_end = byte_idx + ch.len_utf8();
        }
    }
    // Didn't exceed max_chars → no truncation needed.
    Cow::Borrowed(line)
}

/// Post-process the final output string.
///
/// - `strip_empty_lines`: removes blank and whitespace-only lines
/// - `collapse_empty_lines`: collapses consecutive blank lines into one
/// - `truncate_lines_at`: truncate lines longer than N characters
///
/// `strip_empty_lines` takes priority over `collapse_empty_lines` if both are set.
/// `truncate_lines_at` is applied last.
pub fn post_process_output(config: &FilterConfig, output: String) -> String {
    let trailing_newline = output.ends_with('\n');
    let mut result = if config.strip_empty_lines {
        let filtered: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut r = filtered.join("\n");
        if !r.is_empty() && trailing_newline {
            r.push('\n');
        }
        r
    } else if config.collapse_empty_lines {
        let mut r = String::with_capacity(output.len());
        let mut prev_was_empty = false;
        let mut first = true;
        for line in output.lines() {
            let is_empty = line.trim().is_empty();
            if is_empty && prev_was_empty {
                continue;
            }
            if !first {
                r.push('\n');
            }
            r.push_str(line);
            prev_was_empty = is_empty;
            first = false;
        }
        if trailing_newline {
            r.push('\n');
        }
        r
    } else {
        output
    };

    if let Some(max) = config.truncate_lines_at {
        let had_trailing = result.ends_with('\n');
        let mut buf = String::with_capacity(result.len());
        for (i, line) in result.lines().enumerate() {
            if i > 0 {
                buf.push('\n');
            }
            buf.push_str(&truncate_line(line, max));
        }
        if had_trailing {
            buf.push('\n');
        }
        result = buf;
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tokf_common::config::types::FilterConfig;

    fn minimal_config() -> FilterConfig {
        toml::from_str(r#"command = "echo""#).unwrap()
    }

    // --- apply_line_cleanup ---

    #[test]
    fn strip_ansi_removes_color_codes() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        let lines = vec!["\x1b[33mwarning\x1b[0m", "plain text"];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(
            result,
            vec!["warning".to_string(), "plain text".to_string()]
        );
    }

    #[test]
    fn strip_ansi_removes_multi_code_sequences() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        let lines = vec!["\x1b[1;31merror\x1b[0m: \x1b[32msomething\x1b[0m"];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(result, vec!["error: something".to_string()]);
    }

    #[test]
    fn strip_ansi_leaves_plain_text_unchanged() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        let lines = vec!["no escape codes here", "still plain"];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(
            result,
            vec![
                "no escape codes here".to_string(),
                "still plain".to_string()
            ]
        );
    }

    #[test]
    fn trim_lines_removes_leading_trailing_spaces() {
        let mut cfg = minimal_config();
        cfg.trim_lines = true;
        let lines = vec!["  hello  ", "\tworld\t", "  "];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(
            result,
            vec!["hello".to_string(), "world".to_string(), String::new()]
        );
    }

    #[test]
    fn trim_lines_preserves_interior_spaces() {
        let mut cfg = minimal_config();
        cfg.trim_lines = true;
        let lines = vec!["  hello world  "];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(result, vec!["hello world".to_string()]);
    }

    #[test]
    fn no_cleanup_flags_passthrough() {
        let cfg = minimal_config();
        let lines = vec!["\x1b[33mcolored\x1b[0m", "  padded  "];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(
            result,
            vec![
                "\x1b[33mcolored\x1b[0m".to_string(),
                "  padded  ".to_string()
            ]
        );
    }

    #[test]
    fn line_cleanup_empty_input() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        cfg.trim_lines = true;
        let result = apply_line_cleanup(&cfg, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn strip_ansi_and_trim_both_applied() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        cfg.trim_lines = true;
        let lines = vec!["  \x1b[33mwarning\x1b[0m  "];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(result, vec!["warning".to_string()]);
    }

    // --- post_process_output ---

    #[test]
    fn strip_empty_lines_removes_blank_lines() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let output = "line1\n\nline2\n   \nline3".to_string();
        let result = post_process_output(&cfg, output);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn strip_empty_lines_empty_input() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let result = post_process_output(&cfg, String::new());
        assert_eq!(result, "");
    }

    #[test]
    fn strip_empty_lines_all_blank() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let result = post_process_output(&cfg, "\n\n   \n".to_string());
        assert_eq!(result, "");
    }

    #[test]
    fn collapse_empty_lines_reduces_consecutive_blanks() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let output = "line1\n\n\n\nline2".to_string();
        let result = post_process_output(&cfg, output);
        assert_eq!(result, "line1\n\nline2");
    }

    #[test]
    fn collapse_empty_lines_single_blank_unchanged() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let output = "line1\n\nline2".to_string();
        let result = post_process_output(&cfg, output);
        assert_eq!(result, "line1\n\nline2");
    }

    #[test]
    fn collapse_empty_lines_empty_input() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let result = post_process_output(&cfg, String::new());
        assert_eq!(result, "");
    }

    #[test]
    fn strip_empty_beats_collapse_when_both_set() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        cfg.collapse_empty_lines = true;
        let output = "line1\n\n\nline2".to_string();
        // strip_empty_lines takes priority: blank lines are removed entirely
        let result = post_process_output(&cfg, output);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn no_post_process_flags_passthrough() {
        let cfg = minimal_config();
        let output = "line1\n\n\nline2".to_string();
        let result = post_process_output(&cfg, output.clone());
        assert_eq!(result, output);
    }

    #[test]
    fn strip_ansi_removes_osc_hyperlink() {
        let mut cfg = minimal_config();
        cfg.strip_ansi = true;
        // OSC 8 hyperlink: \x1b]8;;url\x1b\\ text \x1b]8;;\x1b\\
        let lines = vec!["\x1b]8;;http://example.com\x1b\\link\x1b]8;;\x1b\\"];
        let result = apply_line_cleanup(&cfg, &lines);
        assert_eq!(result, vec!["link".to_string()]);
    }

    #[test]
    fn strip_empty_lines_preserves_trailing_newline() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let result = post_process_output(&cfg, "line1\n\nline2\n".to_string());
        assert_eq!(result, "line1\nline2\n");
    }

    #[test]
    fn strip_empty_lines_no_trailing_newline_unchanged() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let result = post_process_output(&cfg, "line1\n\nline2".to_string());
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn strip_empty_lines_leading_and_trailing_blank() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        let result = post_process_output(&cfg, "\n\nline1\nline2\n\n".to_string());
        assert_eq!(result, "line1\nline2\n");
    }

    #[test]
    fn collapse_empty_lines_preserves_trailing_newline() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let result = post_process_output(&cfg, "line1\n\n\nline2\n".to_string());
        assert_eq!(result, "line1\n\nline2\n");
    }

    #[test]
    fn collapse_empty_lines_with_leading_blanks() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let result = post_process_output(&cfg, "\n\nline1\nline2".to_string());
        assert_eq!(result, "\nline1\nline2");
    }

    #[test]
    fn collapse_empty_lines_with_whitespace_only_lines() {
        let mut cfg = minimal_config();
        cfg.collapse_empty_lines = true;
        let result = post_process_output(&cfg, "line1\n\t\t\n   \nline2\n\nline3".to_string());
        assert_eq!(result, "line1\n\t\t\nline2\n\nline3");
    }

    // --- truncate_lines_at ---

    #[test]
    fn truncate_lines_at_short_lines_unchanged() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(20);
        let result = post_process_output(&cfg, "short line\nalso ok".to_string());
        assert_eq!(result, "short line\nalso ok");
    }

    #[test]
    fn truncate_lines_at_long_line_truncated() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(10);
        let result = post_process_output(&cfg, "this is a very long line".to_string());
        assert_eq!(result, "this is a\u{2026}");
    }

    #[test]
    fn truncate_lines_at_mixed_lines() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(5);
        let result = post_process_output(&cfg, "hi\nabcdef\nok".to_string());
        assert_eq!(result, "hi\nabcd\u{2026}\nok");
    }

    #[test]
    fn truncate_lines_at_zero() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(0);
        let result = post_process_output(&cfg, "anything".to_string());
        assert_eq!(result, "\u{2026}");
    }

    #[test]
    fn truncate_lines_at_one() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(1);
        let result = post_process_output(&cfg, "abc".to_string());
        assert_eq!(result, "\u{2026}");
    }

    #[test]
    fn truncate_lines_at_preserves_trailing_newline() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(5);
        let result = post_process_output(&cfg, "abcdef\n".to_string());
        assert_eq!(result, "abcd\u{2026}\n");
    }

    #[test]
    fn truncate_lines_at_multibyte_utf8() {
        let mut cfg = minimal_config();
        cfg.truncate_lines_at = Some(4);
        // "日本語です" = 5 chars; truncate to 4 → "日本語" + "…"
        let result = post_process_output(&cfg, "日本語です".to_string());
        assert_eq!(result, "日本語\u{2026}");
    }

    #[test]
    fn truncate_lines_at_with_strip_empty() {
        let mut cfg = minimal_config();
        cfg.strip_empty_lines = true;
        cfg.truncate_lines_at = Some(5);
        let result = post_process_output(&cfg, "abcdef\n\ngh".to_string());
        // strip_empty removes blank line, then truncation: "abcdef" → "abcd…", "gh" stays
        assert_eq!(result, "abcd\u{2026}\ngh");
    }

    // --- defaults ---

    #[test]
    fn all_four_flags_default_false() {
        let cfg = minimal_config();
        assert!(!cfg.strip_ansi);
        assert!(!cfg.trim_lines);
        assert!(!cfg.strip_empty_lines);
        assert!(!cfg.collapse_empty_lines);
    }
}
