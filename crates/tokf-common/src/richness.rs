//! Rarity-weighted retention metric for filter output ("richness").
//!
//! Tokenizes raw and filtered output into *atoms* (whitespace-delimited runs,
//! trimmed of non-alphanumeric edges, at least 6 characters long), weights each
//! **distinct** atom by its self-information `-log2(count / total)`, and reports
//! the fraction of that weight which survived filtering.
//!
//! The weighting is the point: the 400th occurrence of `Compiling` costs almost
//! nothing to drop, while a unique path, hash, or error code costs a lot.
//!
//! **This is never a global gate.** tokf is deliberately lossy — a `cargo check`
//! success collapsing to a one-line summary *should* score near zero, and that
//! is correct. A score only fails a test case when the case explicitly declares
//! `min_richness`.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Minimum atom length, in characters (not bytes).
const MIN_ATOM_CHARS: usize = 6;

/// Rarity-weighted retention score for a single raw/filtered pair.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Richness {
    /// Number of distinct atoms in the raw output.
    pub atoms: usize,
    /// Number of distinct raw atoms that survived into the filtered output.
    pub kept: usize,
    /// Surviving self-information weight divided by total weight, in `0.0..=1.0`.
    pub retained: f64,
}

impl Richness {
    /// The score used when `raw` contains no atoms at all.
    ///
    /// Nothing irreplaceable existed, so nothing could be lost.
    const fn empty() -> Self {
        Self {
            atoms: 0,
            kept: 0,
            retained: 1.0,
        }
    }
}

/// Extract atoms from `text`: whitespace-delimited runs, trimmed of
/// non-alphanumeric edge characters, keeping those of 6+ characters.
///
/// Alphanumeric-ness is Unicode-aware and matching is case-sensitive (hashes
/// and paths are case-significant).
fn atoms(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .map(|tok| tok.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|tok| tok.chars().count() >= MIN_ATOM_CHARS)
}

/// Score how much rarity-weighted information from `raw` survived into `filtered`.
///
/// Returns `retained: 1.0` when `raw` contains no atoms at all — nothing
/// irreplaceable existed, so nothing could be lost. When every occurrence is
/// the same single atom (self-information zero, so the weighted ratio is
/// undefined), the score falls back to the unweighted `kept / atoms` ratio, so
/// dropping that atom entirely still scores `0.0`. Never divides by zero and
/// never returns `NaN`.
#[allow(clippy::cast_precision_loss)]
pub fn score(raw: &str, filtered: &str) -> Richness {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut total = 0_usize;
    for atom in atoms(raw) {
        *counts.entry(atom).or_insert(0) += 1;
        total += 1;
    }

    if total == 0 {
        return Richness::empty();
    }

    let surviving: HashSet<&str> = atoms(filtered).collect();

    let mut total_weight = 0.0_f64;
    let mut kept_weight = 0.0_f64;
    let mut kept = 0_usize;
    for (atom, count) in &counts {
        let weight = -((*count as f64) / (total as f64)).log2();
        total_weight += weight;
        // Substring fallback covers atoms that survive inside a rewritten line
        // rather than as a standalone token. Only reached on a set-lookup miss.
        if surviving.contains(atom) || filtered.contains(*atom) {
            kept += 1;
            kept_weight += weight;
        }
    }

    let atoms = counts.len();
    // Zero total weight means a single distinct atom repeated throughout, whose
    // self-information is zero. The weighted ratio is undefined, so fall back to
    // the unweighted one — which still reports 0.0 if that atom was dropped.
    let retained = if total_weight > 0.0 {
        kept_weight / total_weight
    } else {
        (kept as f64) / (atoms as f64)
    };

    Richness {
        atoms,
        kept,
        retained,
    }
}

/// Check a computed [`Richness`] against an optional declared minimum.
///
/// Returns `None` when no minimum was declared (richness never fails a case on
/// its own) or when the score meets the threshold; otherwise returns a
/// human-readable failure message.
pub fn check_min_richness(min: Option<f64>, r: Richness) -> Option<String> {
    let min = min?;
    if r.retained >= min {
        return None;
    }
    Some(format!(
        "richness {:.3} below min_richness {:.3} ({} of {} distinct atoms retained)",
        r.retained, min, r.kept, r.atoms
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn atom_list(text: &str) -> Vec<&str> {
        atoms(text).collect()
    }

    #[test]
    fn full_retention_scores_one() {
        let raw = "Compiling tokf-common deadbeefcafe src/main.rs failure";
        let r = score(raw, raw);
        assert!(r.atoms > 1, "expected several atoms, got {}", r.atoms);
        assert_eq!(r.kept, r.atoms);
        assert!((r.retained - 1.0).abs() < 1e-9, "retained={}", r.retained);
    }

    #[test]
    fn empty_raw_scores_one() {
        for r in [score("", ""), score("", "anything at all here")] {
            assert_eq!(r.atoms, 0);
            assert_eq!(r.kept, 0);
            assert!(!r.retained.is_nan());
            assert!((r.retained - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn raw_with_no_qualifying_atoms_scores_one() {
        let r = score("a bb ccc dd ee", "");
        assert_eq!(r.atoms, 0);
        assert!((r.retained - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn single_repeated_atom_dropped_scores_zero() {
        // Every occurrence is the same atom => p == 1.0 => -log2(1.0) == 0, so
        // the weighted ratio is undefined. Falling back to kept/atoms keeps the
        // "everything was swallowed" case honest instead of reporting 1.0.
        let r = score("Compiling Compiling Compiling", "");
        assert_eq!(r.atoms, 1);
        assert_eq!(r.kept, 0);
        assert!(!r.retained.is_nan());
        assert!(r.retained.abs() < f64::EPSILON, "retained={}", r.retained);
    }

    #[test]
    fn single_repeated_atom_kept_scores_one() {
        let r = score("Compiling Compiling Compiling", "Compiling");
        assert_eq!(r.atoms, 1);
        assert_eq!(r.kept, 1);
        assert!(!r.retained.is_nan());
        assert!((r.retained - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn dropping_rare_atom_costs_more_than_dropping_common_one() {
        let mut raw = String::new();
        for _ in 0..20 {
            raw.push_str("Compiling ");
        }
        raw.push_str("deadbeefcafe");

        // A: drops the rare atom, keeps the common one.
        let a = score(&raw, "Compiling");
        // B: drops all the common ones, keeps the rare atom.
        let b = score(&raw, "deadbeefcafe");

        assert!(
            a.retained < b.retained,
            "dropping the rare atom ({}) should cost more than dropping the common one ({})",
            a.retained,
            b.retained
        );
    }

    #[test]
    fn keeping_nothing_scores_near_zero() {
        let raw = "Compiling tokf-common deadbeefcafe src/main.rs panicked assertion failed";
        let r = score(raw, "");
        assert!(r.retained >= 0.0, "retained={}", r.retained);
        assert!(r.retained < 0.05, "retained={}", r.retained);
        assert_eq!(r.kept, 0);
    }

    #[test]
    fn atom_extraction_trims_non_alphanumeric_edges() {
        assert_eq!(atom_list("(hello_world),"), vec!["hello_world"]);
        // Interior punctuation is preserved.
        assert_eq!(atom_list("(src/main.rs):"), vec!["src/main.rs"]);
        // Too short after trimming.
        assert!(atom_list("(abc),").is_empty());
    }

    #[test]
    fn atom_extraction_min_length_is_chars_not_bytes() {
        // 5 chars, 10 bytes -> excluded.
        assert!(atom_list("\u{3b1}\u{3b1}\u{3b1}\u{3b1}\u{3b1}").is_empty());
        // 6 chars -> included.
        assert_eq!(
            atom_list("\u{3b1}\u{3b1}\u{3b1}\u{3b1}\u{3b1}\u{3b1}").len(),
            1
        );
    }

    #[test]
    fn atom_matching_is_case_sensitive() {
        let r = score("DEADBEEFCAFE unrelated", "deadbeefcafe unrelated");
        assert_eq!(r.atoms, 2);
        assert_eq!(r.kept, 1);
    }

    #[test]
    fn substring_fallback_counts_rewritten_atoms() {
        let r = score("src/lib/module.rs", "at src/lib/module.rs:42:");
        assert_eq!(r.atoms, 1);
        assert_eq!(r.kept, 1);
    }

    #[test]
    fn check_min_richness_none_never_fails() {
        // Anti-global-gate guarantee: no declaration means no failure, ever.
        let r = Richness {
            atoms: 100,
            kept: 0,
            retained: 0.0,
        };
        assert!(check_min_richness(None, r).is_none());
    }

    #[test]
    fn check_min_richness_passes_at_exact_threshold() {
        let r = Richness {
            atoms: 10,
            kept: 4,
            retained: 0.4,
        };
        assert!(check_min_richness(Some(0.4), r).is_none());
    }

    #[test]
    fn check_min_richness_message_mentions_numbers() {
        let r = Richness {
            atoms: 97,
            kept: 12,
            retained: 0.31,
        };
        let msg = check_min_richness(Some(0.5), r).unwrap();
        assert!(msg.contains("min_richness"), "{msg}");
        assert!(msg.contains("0.310"), "{msg}");
        assert!(msg.contains("12 of 97"), "{msg}");
    }
}
