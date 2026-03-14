use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Interesting,
    AlwaysKeep,
    Normal,
}

pub struct PatternMatcher {
    interesting: Vec<Regex>,
    always_keep: Vec<Regex>,
}

impl PatternMatcher {
    pub fn new(interesting: &[&str], always_keep: &[&str]) -> Self {
        Self {
            interesting: interesting
                .iter()
                .filter_map(|p| Regex::new(p).ok())
                .collect(),
            always_keep: always_keep
                .iter()
                .filter_map(|p| Regex::new(p).ok())
                .collect(),
        }
    }

    pub fn classify(&self, line: &str) -> LineKind {
        if self.always_keep.iter().any(|r| r.is_match(line)) {
            return LineKind::AlwaysKeep;
        }
        if self.interesting.iter().any(|r| r.is_match(line)) {
            return LineKind::Interesting;
        }
        LineKind::Normal
    }

    pub fn mark_lines(&self, lines: &[&str]) -> Vec<LineKind> {
        lines.iter().map(|l| self.classify(l)).collect()
    }
}

/// Extract interesting lines with surrounding context, inserting `...` between
/// non-contiguous regions. `AlwaysKeep` lines are always included.
pub fn extract_with_context(text: &str, matcher: &PatternMatcher, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let kinds = matcher.mark_lines(&lines);

    // Build a boolean mask of which lines to include.
    let mut include = vec![false; lines.len()];

    for (i, kind) in kinds.iter().enumerate() {
        match kind {
            LineKind::Interesting => {
                let start = i.saturating_sub(context);
                let end = (i + context + 1).min(lines.len());
                for slot in &mut include[start..end] {
                    *slot = true;
                }
            }
            LineKind::AlwaysKeep => {
                include[i] = true;
            }
            LineKind::Normal => {}
        }
    }

    let mut result = Vec::new();
    let mut last_included: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        if !include[i] {
            continue;
        }
        if last_included.is_some_and(|prev| i > prev + 1) {
            result.push("...");
        }
        result.push(line);
        last_included = Some(i);
    }

    result.join("\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn classify_interesting() {
        let m = PatternMatcher::new(&["^error"], &[]);
        assert_eq!(m.classify("error: something"), LineKind::Interesting);
        assert_eq!(m.classify("ok line"), LineKind::Normal);
    }

    #[test]
    fn classify_always_keep() {
        let m = PatternMatcher::new(&["^error"], &["^summary"]);
        assert_eq!(m.classify("summary: 5 passed"), LineKind::AlwaysKeep);
    }

    #[test]
    fn extract_with_context_basic() {
        let text = "line 1\nline 2\nerror: bad\nline 4\nline 5\nline 6\nline 7";
        let m = PatternMatcher::new(&["^error"], &[]);
        let result = extract_with_context(text, &m, 1);
        assert_eq!(result, "line 2\nerror: bad\nline 4");
    }

    #[test]
    fn extract_with_context_gap() {
        let text = "error: first\nok 1\nok 2\nok 3\nok 4\nerror: second";
        let m = PatternMatcher::new(&["^error"], &[]);
        let result = extract_with_context(text, &m, 1);
        assert_eq!(result, "error: first\nok 1\n...\nok 4\nerror: second");
    }

    #[test]
    fn extract_always_keep_included() {
        let text = "line 1\nline 2\nline 3\nsummary: done";
        let m = PatternMatcher::new(&["^error"], &["^summary"]);
        let result = extract_with_context(text, &m, 1);
        assert_eq!(result, "summary: done");
    }

    #[test]
    fn extract_empty_input() {
        let m = PatternMatcher::new(&["^error"], &[]);
        assert_eq!(extract_with_context("", &m, 3), "");
    }

    #[test]
    fn extract_no_matches_returns_empty() {
        let text = "all good\nno issues\nfine";
        let m = PatternMatcher::new(&["^error"], &[]);
        assert_eq!(extract_with_context(text, &m, 1), "");
    }
}
