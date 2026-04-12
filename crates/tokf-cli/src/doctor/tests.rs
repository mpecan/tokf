#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_arguments)]

use super::*;

// ─────────────────────────── score_filter ───────────────────────────

fn score(
    burst_events: usize,
    events: usize,
    fail_ratio: f64,
    wk: usize,
    chain: usize,
    excess: Option<f64>,
    pipe: f64,
) -> u8 {
    score_filter(&ScoreInput {
        total_burst_events: burst_events,
        event_count: events,
        failed_burst_ratio: fail_ratio,
        workaround_count: wk,
        max_empty_chain: chain,
        avg_excess_tokens: excess,
        pipe_override_rate: pipe,
    })
}

#[test]
fn score_perfect_filter_is_100() {
    assert_eq!(score(0, 100, 0.0, 0, 0, None, 0.0), 100);
    assert_eq!(score(0, 100, 0.0, 0, 0, Some(-50.0), 0.0), 100);
}

#[test]
fn score_drops_with_burst_rate() {
    let baseline = score(0, 100, 0.0, 0, 0, None, 0.0);
    let with_bursts = score(50, 100, 0.0, 0, 0, None, 0.0);
    assert!(with_bursts < baseline);
}

#[test]
fn score_high_volume_low_burst_rate_is_healthy() {
    let s = score(16, 13682, 0.0, 0, 0, None, 0.0);
    assert!(
        s >= 95,
        "high-volume low-burst-rate should score well, got {s}"
    );
}

#[test]
fn score_low_volume_high_burst_rate_is_bad() {
    // 15/20 = 75% rate → capped at 30 penalty → score 70.
    let s = score(15, 20, 0.0, 0, 0, None, 0.0);
    assert!(
        s <= 75,
        "low-volume high-burst-rate should score poorly, got {s}"
    );
}

#[test]
fn score_failed_bursts_penalized_harder() {
    // Use a rate low enough that the failure multiplier matters before cap.
    // 10/100 = 10% → no_fail penalty = round(10*1.0)=10, all_fail = round(10*2.0)=20.
    let no_fail = score(10, 100, 0.0, 0, 0, None, 0.0);
    let all_fail = score(10, 100, 1.0, 0, 0, None, 0.0);
    assert!(
        all_fail < no_fail,
        "all-fail bursts should be worse: no_fail={no_fail}, all_fail={all_fail}"
    );
}

#[test]
fn score_caps_burst_penalty_at_30() {
    let s = score(100, 100, 0.0, 0, 0, None, 0.0);
    assert_eq!(s, 70, "burst penalty should max out at 30");
}

#[test]
fn score_caps_workaround_penalty_at_15() {
    let s = score(0, 100, 0.0, 100, 0, None, 0.0);
    assert_eq!(s, 85);
}

#[test]
fn score_empty_chain_penalty() {
    // Chain of 5 → 5*2 = 10 pts penalty
    let s = score(0, 100, 0.0, 0, 5, None, 0.0);
    assert_eq!(s, 90);
    // Chain of 10+ → capped at 15
    let s2 = score(0, 100, 0.0, 0, 20, None, 0.0);
    assert_eq!(s2, 85);
}

#[test]
fn score_pipe_override_penalty() {
    // 10% override rate → 10 pts
    let s = score(0, 100, 0.0, 0, 0, None, 0.10);
    assert_eq!(s, 90);
    // 50%+ → capped at 10
    let s2 = score(0, 100, 0.0, 0, 0, None, 0.50);
    assert_eq!(s2, 90);
}

#[test]
fn score_combines_all_signals() {
    // Max all five: 30 + 15 + 15 + 15 + 10 = 85 penalty → score 15.
    let s = score(100, 100, 1.0, 100, 20, Some(1000.0), 1.0);
    assert_eq!(s, 15);
}

#[test]
fn score_is_monotonic_in_burst_rate() {
    let mut prev = score(0, 100, 0.0, 0, 0, None, 0.0);
    for n in 1..=100 {
        let cur = score(n, 100, 0.0, 0, 0, None, 0.0);
        assert!(cur <= prev, "monotonic at n={n}");
        prev = cur;
    }
}

#[test]
fn score_zero_events_does_not_panic() {
    assert_eq!(score(0, 0, 0.0, 0, 0, None, 0.0), 100);
}

// ─────────────────────────── sort_reports ───────────────────────────

fn make_report(name: &str, health: u8, bursts: usize) -> FilterReport {
    FilterReport {
        filter_name: name.to_string(),
        event_count: 10,
        burst_count: bursts,
        max_burst_size: bursts,
        failed_burst_ratio: 0.0,
        shape_burst_count: 0,
        median_arg_uniqueness: None,
        untracked_workaround_flags: vec![],
        empty_chain_count: 0,
        max_empty_chain: 0,
        avg_excess_tokens: None,
        pipe_override_rate: 0.0,
        burst_time_wasted_ms: 0,
        health_score: health,
    }
}

#[test]
fn sort_health_ascending() {
    let mut reports = vec![
        make_report("a", 80, 0),
        make_report("b", 30, 5),
        make_report("c", 60, 1),
    ];
    sort_reports(&mut reports, SortBy::Health);
    assert_eq!(reports[0].filter_name, "b");
    assert_eq!(reports[2].filter_name, "a");
}

#[test]
fn sort_health_breaks_ties_by_name() {
    let mut reports = vec![make_report("zebra", 50, 1), make_report("alpha", 50, 1)];
    sort_reports(&mut reports, SortBy::Health);
    assert_eq!(reports[0].filter_name, "alpha");
}

// ─────────────────────── shape_median_uniqueness ─────────────────────

#[test]
fn shape_median_none_for_empty() {
    assert!(shape_median_uniqueness(&[]).is_none());
}

#[test]
fn shape_median_single_burst() {
    let b = queries::ShapeBurstRow {
        filter_name: "f".to_string(),
        shape: "s".to_string(),
        burst_size: 10,
        distinct_commands: 5,
        arg_uniqueness: 0.5,
        failed_count: 0,
        last_seen: "t".to_string(),
    };
    let m = shape_median_uniqueness(&[&b]).unwrap();
    assert!((m - 0.5).abs() < 0.001);
}
