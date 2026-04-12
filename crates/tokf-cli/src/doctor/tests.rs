#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

// ─────────────────────────── score_filter ───────────────────────────

#[test]
fn score_perfect_filter_is_100() {
    assert_eq!(score_filter(0, 0, 0, None), 100);
    assert_eq!(score_filter(0, 0, 0, Some(-50.0)), 100);
}

#[test]
fn score_drops_with_bursts() {
    let baseline = score_filter(0, 0, 0, None);
    let with_bursts = score_filter(5, 0, 0, None);
    assert!(with_bursts < baseline);
}

#[test]
fn score_caps_burst_penalty_at_40() {
    // 100 bursts × 2 = 200 → capped at 40
    let s = score_filter(100, 0, 0, None);
    assert_eq!(s, 60, "burst penalty should max out at 40");
}

#[test]
fn score_caps_workaround_penalty_at_20() {
    let s = score_filter(0, 100, 0, None);
    assert_eq!(s, 80);
}

#[test]
fn score_caps_empty_retry_penalty_at_20() {
    let s = score_filter(0, 0, 100, None);
    assert_eq!(s, 80);
}

#[test]
fn score_caps_negative_savings_at_20() {
    let s = score_filter(0, 0, 0, Some(1000.0));
    assert_eq!(s, 80);
}

#[test]
fn score_combines_all_signals() {
    // Max all four → 0
    let s = score_filter(100, 100, 100, Some(1000.0));
    assert_eq!(s, 0);
}

#[test]
fn score_is_monotonic_in_burst_count() {
    let mut prev = score_filter(0, 0, 0, None);
    for n in 1..=20 {
        let cur = score_filter(n, 0, 0, None);
        assert!(
            cur <= prev,
            "score must be monotonically non-increasing in burst count"
        );
        prev = cur;
    }
}

#[test]
fn score_is_monotonic_in_workaround_count() {
    let mut prev = score_filter(0, 0, 0, None);
    for n in 1..=20 {
        let cur = score_filter(0, n, 0, None);
        assert!(cur <= prev);
        prev = cur;
    }
}

#[test]
fn score_negative_savings_doesnt_double_count_when_negative() {
    // Negative excess (filter is saving tokens) should give zero penalty.
    let s = score_filter(0, 0, 0, Some(-100.0));
    assert_eq!(s, 100);
}

// ─────────────────────────── sort_reports ───────────────────────────

fn make_report(name: &str, health: u8, bursts: usize, max_burst: usize) -> FilterReport {
    FilterReport {
        filter_name: name.to_string(),
        event_count: 10,
        burst_count: bursts,
        max_burst_size: max_burst,
        median_arg_uniqueness: None,
        untracked_workaround_flags: vec![],
        empty_retry_count: 0,
        avg_excess_tokens: None,
        health_score: health,
    }
}

#[test]
fn sort_health_ascending() {
    let mut reports = vec![
        make_report("a", 80, 0, 0),
        make_report("b", 30, 5, 5),
        make_report("c", 60, 1, 1),
    ];
    sort_reports(&mut reports, SortBy::Health);
    assert_eq!(reports[0].filter_name, "b"); // worst (30)
    assert_eq!(reports[1].filter_name, "c"); // 60
    assert_eq!(reports[2].filter_name, "a"); // best (80)
}

#[test]
fn sort_bursts_descending() {
    let mut reports = vec![
        make_report("a", 80, 0, 0),
        make_report("b", 30, 5, 5),
        make_report("c", 60, 2, 3),
    ];
    sort_reports(&mut reports, SortBy::Bursts);
    assert_eq!(reports[0].filter_name, "b");
    assert_eq!(reports[1].filter_name, "c");
    assert_eq!(reports[2].filter_name, "a");
}

#[test]
fn sort_health_breaks_ties_by_name() {
    let mut reports = vec![
        make_report("zebra", 50, 1, 1),
        make_report("alpha", 50, 1, 1),
    ];
    sort_reports(&mut reports, SortBy::Health);
    assert_eq!(reports[0].filter_name, "alpha");
    assert_eq!(reports[1].filter_name, "zebra");
}

// ─────────────────────────── median_uniqueness ───────────────────────

#[test]
fn median_uniqueness_none_for_empty_bursts() {
    assert!(median_uniqueness(&[]).is_none());
}

#[test]
fn median_uniqueness_single_burst() {
    let b = queries::BurstRow {
        filter_name: "f".to_string(),
        command: "cmd".to_string(),
        burst_size: 10,
        last_seen: "t".to_string(),
    };
    let m = median_uniqueness(&[&b]).unwrap();
    assert!((m - 0.1).abs() < 0.001, "expected ~0.1, got {m}");
}

#[test]
fn median_uniqueness_odd_count() {
    let b1 = queries::BurstRow {
        filter_name: "f".to_string(),
        command: "a".to_string(),
        burst_size: 5,
        last_seen: "t".to_string(),
    };
    let b2 = queries::BurstRow {
        filter_name: "f".to_string(),
        command: "b".to_string(),
        burst_size: 10,
        last_seen: "t".to_string(),
    };
    let b3 = queries::BurstRow {
        filter_name: "f".to_string(),
        command: "c".to_string(),
        burst_size: 20,
        last_seen: "t".to_string(),
    };
    // ratios: 1/5=0.2, 1/10=0.1, 1/20=0.05 → sorted: 0.05, 0.1, 0.2 → median=0.1
    let m = median_uniqueness(&[&b1, &b2, &b3]).unwrap();
    assert!((m - 0.1).abs() < 0.001, "expected 0.1, got {m}");
}
