//! Token counting.
//!
//! tokf reports token counts in `tokf gain`, persists them to `SQLite`, and
//! syncs them to the server. Those numbers come from a cheap byte heuristic —
//! `bytes / DIVISOR` — not from a real tokenizer. The default build takes no
//! tokenizer dependency: counting tokens exactly is not worth a vocab table in
//! the shipping binary just to serve a statistic.
//!
//! Because it is a heuristic, every user-facing surface that prints these
//! numbers must keep labelling them `est.`.
//!
//! The heuristic can be *verified* against a real cl100k tokenizer via the
//! optional, off-by-default `tokenizer` feature (see
//! `crates/tokf-cli/tests/calibration.rs`). cl100k is not Claude's tokenizer,
//! so even that is an approximation — a calibration target, not truth.

/// Bytes per estimated token.
///
/// This is a heuristic, not a measurement — but it is a *calibrated* one.
/// Measured against a real cl100k tokenizer over the whole tokf corpus
/// (every filter `_test/` case and every file under `tests/fixtures/`,
/// before and after filtering), the corpus implies 3.53 bytes per token
/// overall: 3.67 on raw command output, 2.98 on filtered output, with a
/// per-item spread of p10 2.72 / median 3.39 / p90 4.62. 3.5 is that
/// combined figure rounded; the previous value of 4.0 systematically
/// undercounted. See `docs/token-tracking.md` for the caveats.
///
/// Reproduce with:
/// `cargo test -p tokf --features tokenizer --test calibration -- --ignored --nocapture`
///
/// NOTE: while [`ArithmeticTokenCounter`] is the shipping default, every
/// user-facing surface that prints these numbers (notably `tokf gain`) must
/// keep labelling them `est.`. Recalibrating removed a bias; it did not make
/// these counts exact, and the spread above shows one constant cannot.
pub const DIVISOR: f64 = 3.5;

/// Something that can count the tokens in a string.
///
/// Object-safe on purpose: call sites take `&dyn TokenCounter` so the
/// estimator is swappable rather than hardcoded arithmetic.
pub trait TokenCounter {
    /// Count the tokens in `text`.
    fn count(&self, text: &str) -> usize;
}

/// The default, dependency-free estimator: `bytes / DIVISOR`, truncated.
///
/// Counts *bytes*, not characters — multi-byte UTF-8 therefore reads as more
/// tokens than the same number of ASCII characters would.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArithmeticTokenCounter;

impl TokenCounter for ArithmeticTokenCounter {
    fn count(&self, text: &str) -> usize {
        estimate_tokens_from_bytes(text.len())
    }
}

/// Estimate tokens for a string using the default arithmetic estimator.
pub fn estimate_tokens(s: &str) -> usize {
    estimate_tokens_from_bytes(s.len())
}

/// Estimate tokens from a byte count.
///
/// Exists so callers that only ever had byte counts (the tracking and
/// telemetry event builders) share the exact same arithmetic as the
/// string-based entry point instead of re-deriving it.
///
/// Truncates, matching the integer-division semantics this estimator has
/// always had. `DIVISOR` is an `f64` so it can later be a non-integer; the
/// single cast `#[allow]` lives here and nowhere else, so call sites stay
/// free of casting lints.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
pub fn estimate_tokens_from_bytes(bytes: usize) -> usize {
    (bytes as f64 / DIVISOR) as usize
}

/// A real cl100k tokenizer, for verifying and calibrating the estimator.
///
/// Available only under the optional, off-by-default `tokenizer` feature.
/// Nothing in the shipping runtime path may use this — it exists so we can
/// measure how wrong [`ArithmeticTokenCounter`] is, and re-check that later.
#[cfg(feature = "tokenizer")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cl100kTokenCounter;

#[cfg(feature = "tokenizer")]
impl TokenCounter for Cl100kTokenCounter {
    fn count(&self, text: &str) -> usize {
        // `cl100k_base()` returns a process-wide static; construction is free.
        bpe_openai::cl100k_base().count(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(ArithmeticTokenCounter.count(""), 0);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens_from_bytes(0), 0);
    }

    #[test]
    fn shorter_than_divisor_truncates_to_zero() {
        // Truncation, not rounding — this is the historical behaviour and an
        // easy accidental regression if someone reaches for `round()`.
        assert_eq!(estimate_tokens_from_bytes(1), 0);
    }

    #[test]
    fn scales_linearly_with_the_constant() {
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let expected = (4000.0_f64 / DIVISOR) as usize;
        assert_eq!(estimate_tokens_from_bytes(4000), expected);
        assert_eq!(ArithmeticTokenCounter.count(&"a".repeat(4000)), expected);
    }

    #[test]
    fn string_and_byte_entry_points_agree() {
        for s in ["", "a", "hello world", &"x".repeat(1234)] {
            assert_eq!(estimate_tokens(s), estimate_tokens_from_bytes(s.len()));
            assert_eq!(ArithmeticTokenCounter.count(s), estimate_tokens(s));
        }
    }

    #[test]
    fn counts_bytes_not_chars() {
        // "é" is two bytes; the heuristic is deliberately byte-based.
        let s = "é".repeat(100);
        assert_eq!(s.chars().count(), 100);
        assert_eq!(estimate_tokens(&s), estimate_tokens_from_bytes(200));
    }

    #[test]
    fn very_large_byte_counts_do_not_panic() {
        let n = estimate_tokens_from_bytes(usize::MAX);
        assert!(n > 0);
    }

    #[test]
    fn counter_is_object_safe() {
        let c: &dyn TokenCounter = &ArithmeticTokenCounter;
        assert_eq!(c.count("hello world"), estimate_tokens("hello world"));
    }

    #[cfg(feature = "tokenizer")]
    #[test]
    fn cl100k_matches_hand_verified_counts() {
        let c = Cl100kTokenCounter;
        assert_eq!(c.count(""), 0);
        // Hand-verified against the cl100k_base vocabulary.
        assert_eq!(c.count("hello world"), 2);
        assert_eq!(c.count("héllo wörld"), 6);
    }

    #[cfg(feature = "tokenizer")]
    #[test]
    fn cl100k_is_object_safe_too() {
        let c: &dyn TokenCounter = &Cl100kTokenCounter;
        assert!(c.count("fn main() {}") > 0);
    }
}
