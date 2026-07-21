//! Byte-stability check for filter output.
//!
//! `tokf verify` runs each test case's filter pipeline twice against the
//! same input and asserts the two outputs are byte-identical — a filter is
//! required to be a pure function of its input. See
//! `docs/writing-filters.md#determinism` for the rationale (prompt-cache
//! invalidation on drift) and the per-process `HashMap`-seed trap this
//! guards against.

/// Compare two independent filter runs over the same input and return a
/// failure message if they diverge. Returns `None` when the outputs are
/// byte-identical (the expected, invariant case).
pub(super) fn check(filter_name: &str, first: &str, second: &str) -> Option<String> {
    if first == second {
        return None;
    }
    Some(format_failure(filter_name, first, second))
}

/// Number of bytes of context shown on each side of the first differing byte.
const DIFF_CONTEXT_BYTES: usize = 20;

/// Return the byte offset of the first point at which `a` and `b` diverge.
///
/// If one string is a strict prefix of the other, the offset is the length
/// of the shorter string. Returns `None` only when the two strings are
/// byte-identical.
fn first_diff_offset(a: &str, b: &str) -> Option<usize> {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    let min_len = a_bytes.len().min(b_bytes.len());
    if let Some(pos) = a_bytes[..min_len]
        .iter()
        .zip(&b_bytes[..min_len])
        .position(|(x, y)| x != y)
    {
        return Some(pos);
    }
    if a_bytes.len() == b_bytes.len() {
        None
    } else {
        Some(min_len)
    }
}

/// Snap a byte offset outward to the nearest valid `char` boundary so a
/// slice starting or ending there never panics on a multi-byte character.
/// `forward` controls which direction to snap when `offset` sits inside a
/// multi-byte sequence.
fn snap_to_char_boundary(s: &str, offset: usize, forward: bool) -> usize {
    let mut i = offset.min(s.len());
    if forward {
        while i < s.len() && !s.is_char_boundary(i) {
            i += 1;
        }
    } else {
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
    }
    i
}

/// A boundary-safe window of `s` around byte `offset`, `DIFF_CONTEXT_BYTES`
/// bytes on either side.
fn context_window(s: &str, offset: usize) -> &str {
    let start = snap_to_char_boundary(s, offset.saturating_sub(DIFF_CONTEXT_BYTES), false);
    let end = snap_to_char_boundary(s, offset.saturating_add(DIFF_CONTEXT_BYTES), true);
    &s[start..end]
}

/// Build a human-readable determinism-failure message naming the filter and
/// the first differing byte offset, with surrounding context from both runs.
fn format_failure(filter_name: &str, first: &str, second: &str) -> String {
    let Some(offset) = first_diff_offset(first, second) else {
        // Unreachable in practice: callers only invoke this on unequal
        // strings. Keep the function total rather than panicking.
        return format!(
            "{filter_name}: output is not byte-stable across repeated runs \
             (could not locate a differing byte)"
        );
    };
    format!(
        "{filter_name}: output is not byte-stable across repeated runs \
         (first differing byte at offset {offset})\n\
         \x20   run 1: ...{:?}...\n\
         \x20   run 2: ...{:?}...",
        context_window(first, offset),
        context_window(second, offset),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // --- first_diff_offset ---

    #[test]
    fn first_diff_offset_identical_returns_none() {
        assert_eq!(first_diff_offset("same text", "same text"), None);
        assert_eq!(first_diff_offset("", ""), None);
    }

    #[test]
    fn first_diff_offset_interior_difference() {
        assert_eq!(first_diff_offset("hello world", "hello WORLD"), Some(6));
    }

    #[test]
    fn first_diff_offset_prefix_mismatch_uses_shorter_length() {
        // "abc" is a strict prefix of "abcdef" — offset is len("abc") == 3,
        // not None (they are NOT identical) and not a panic.
        assert_eq!(first_diff_offset("abc", "abcdef"), Some(3));
        assert_eq!(first_diff_offset("abcdef", "abc"), Some(3));
    }

    #[test]
    fn first_diff_offset_first_byte_differs() {
        assert_eq!(first_diff_offset("a", "b"), Some(0));
    }

    // --- char-boundary safety ---

    #[test]
    fn context_window_never_panics_near_multibyte_boundary() {
        // "\u{2014}" (em dash) and "\u{2713}" (check mark) are both
        // multi-byte in UTF-8; the diff offset can legitimately land inside
        // one of these sequences and must not slice mid-character.
        let a = "clean \u{2014} nothing to commit \u{2713} done";
        let b = "clean \u{2014} nothing to change \u{2713} done";
        let offset = first_diff_offset(a, b).expect("strings differ");
        // Must not panic, and must return valid UTF-8 slices.
        let win_a = context_window(a, offset);
        let win_b = context_window(b, offset);
        assert!(std::str::from_utf8(win_a.as_bytes()).is_ok());
        assert!(std::str::from_utf8(win_b.as_bytes()).is_ok());
    }

    #[test]
    fn context_window_near_start_and_end_does_not_panic() {
        let s = "\u{2713}\u{2713}\u{2713}";
        // offset 0 and offset at end are edge cases for saturating arithmetic.
        let _ = context_window(s, 0);
        let _ = context_window(s, s.len());
    }

    #[test]
    fn snap_to_char_boundary_snaps_forward_and_backward() {
        let s = "a\u{2014}b"; // 'a' (1 byte), em-dash (3 bytes), 'b' (1 byte)
        // Byte 2 is inside the em-dash (bytes 1..4).
        assert!(!s.is_char_boundary(2));
        assert_eq!(snap_to_char_boundary(s, 2, true), 4);
        assert_eq!(snap_to_char_boundary(s, 2, false), 1);
    }

    // --- check / format_failure ---

    #[test]
    fn check_identical_outputs_is_none() {
        assert_eq!(check("git/status", "same", "same"), None);
    }

    #[test]
    fn check_differing_outputs_names_filter_and_offset() {
        let msg = check("git/status", "main\nclean", "main\ndirty")
            .expect("outputs differ, expected a failure message");
        assert!(
            msg.contains("git/status"),
            "expected filter name in message: {msg}"
        );
        assert!(
            msg.contains("offset 5"),
            "expected byte offset 5 in message: {msg}"
        );
    }

    #[test]
    fn format_failure_includes_context_from_both_runs() {
        let msg = format_failure("cargo/test", "aaaXbbb", "aaaYbbb");
        assert!(msg.contains("cargo/test"));
        assert!(msg.contains("offset 3"));
        assert!(msg.contains("run 1"));
        assert!(msg.contains("run 2"));
    }
}
