use std::collections::VecDeque;

/// Collapse duplicate lines within a sliding window.
///
/// - `window = None` — drop lines identical to the immediately preceding line
/// - `window = Some(n)` — drop lines that appear anywhere in the last `n` output lines
///
/// Returns a filtered vec of references into the input slice.
pub fn apply_dedup<'a>(lines: &[&'a str], window: Option<usize>) -> Vec<&'a str> {
    window.map_or_else(|| dedup_consecutive(lines), |n| dedup_windowed(lines, n))
}

fn dedup_consecutive<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut result: Vec<&'a str> = Vec::with_capacity(lines.len());
    for &line in lines {
        if result.last().copied() != Some(line) {
            result.push(line);
        }
    }
    result
}

fn dedup_windowed<'a>(lines: &[&'a str], window: usize) -> Vec<&'a str> {
    let mut result: Vec<&'a str> = Vec::with_capacity(lines.len());
    // Ring buffer of the last `window` output lines for fast lookup.
    let mut recent: VecDeque<&'a str> = VecDeque::with_capacity(window);
    for &line in lines {
        if recent.contains(&line) {
            continue;
        }
        result.push(line);
        if recent.len() == window {
            recent.pop_front();
        }
        recent.push_back(line);
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn dedup_empty_input() {
        let result = apply_dedup(&[], None);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_no_consecutive() {
        let lines = vec!["a", "b", "c"];
        assert_eq!(apply_dedup(&lines, None), vec!["a", "b", "c"]);
    }

    #[test]
    fn dedup_consecutive_collapsed() {
        let lines = vec!["a", "a", "b", "b", "b", "a"];
        assert_eq!(apply_dedup(&lines, None), vec!["a", "b", "a"]);
    }

    #[test]
    fn dedup_non_consecutive_kept() {
        // Default (no window): non-adjacent duplicates are kept.
        let lines = vec!["a", "b", "a"];
        assert_eq!(apply_dedup(&lines, None), vec!["a", "b", "a"]);
    }

    #[test]
    fn dedup_window_drops_within_window() {
        let lines = vec!["a", "b", "a"];
        assert_eq!(apply_dedup(&lines, Some(3)), vec!["a", "b"]);
    }

    #[test]
    fn dedup_window_keeps_beyond_window() {
        // window=2: "a" drops once "b","c" push it out
        let lines = vec!["a", "b", "c", "a"];
        assert_eq!(apply_dedup(&lines, Some(2)), vec!["a", "b", "c", "a"]);
    }

    #[test]
    fn dedup_single_line() {
        let lines = vec!["only"];
        assert_eq!(apply_dedup(&lines, None), vec!["only"]);
    }
}
