#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::super::{DoctorReport, FilterReport, WorkaroundFlagSuggestion};
use super::*;

fn empty_report() -> DoctorReport {
    DoctorReport {
        total_events_considered: 0,
        project_filter: None,
        include_noise: false,
        burst_threshold: 5,
        window_secs: 60,
        filters: vec![],
        bursts: vec![],
        empty_retries: vec![],
        negative_savings: vec![],
        workaround_flags: vec![],
    }
}

fn report_with_one_filter() -> DoctorReport {
    DoctorReport {
        total_events_considered: 50,
        project_filter: Some("/Users/x/repo".to_string()),
        include_noise: false,
        burst_threshold: 5,
        window_secs: 60,
        filters: vec![FilterReport {
            filter_name: "git/diff".to_string(),
            event_count: 30,
            burst_count: 3,
            max_burst_size: 12,
            untracked_workaround_flags: vec![WorkaroundFlagSuggestion {
                flag: "--no-stat".to_string(),
                count: 8,
            }],
            empty_retry_count: 2,
            avg_excess_tokens: Some(15.0),
            health_score: 30,
        }],
        bursts: vec![super::super::queries::BurstRow {
            filter_name: "git/diff".to_string(),
            command: "git diff".to_string(),
            burst_size: 12,
            last_seen: "2024-01-01T00:00:30Z".to_string(),
        }],
        empty_retries: vec![],
        negative_savings: vec![],
        workaround_flags: vec![],
    }
}

#[test]
fn human_empty_db_friendly_message() {
    let report = empty_report();
    let out = render_human(&report, &Colors::disabled());
    assert!(out.contains("no events yet"));
}

#[test]
fn human_renders_filter_table() {
    let report = report_with_one_filter();
    let out = render_human(&report, &Colors::disabled());
    assert!(
        out.contains("git/diff"),
        "should contain filter name: {out}"
    );
    assert!(out.contains("score"), "should contain table header: {out}");
    assert!(out.contains("30"), "should contain event count");
}

#[test]
fn human_shows_burst_detail_when_present() {
    let report = report_with_one_filter();
    let out = render_human(&report, &Colors::disabled());
    assert!(out.contains("retry-burst detail"));
    assert!(out.contains("×12"));
}

#[test]
fn human_shows_workaround_suggestions() {
    let report = report_with_one_filter();
    let out = render_human(&report, &Colors::disabled());
    assert!(out.contains("workaround-flag suggestions"));
    assert!(out.contains("--no-stat×8"));
}

#[test]
fn human_shows_negative_savings_callout() {
    let report = report_with_one_filter();
    let out = render_human(&report, &Colors::disabled());
    assert!(out.contains("negative token savings"));
    assert!(out.contains("git/diff"));
}

#[test]
fn human_no_negative_section_when_none() {
    let mut report = report_with_one_filter();
    report.filters[0].avg_excess_tokens = None;
    let out = render_human(&report, &Colors::disabled());
    assert!(!out.contains("negative token savings"));
}

#[test]
fn json_round_trip() {
    let report = report_with_one_filter();
    let json = serde_json::to_string(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["total_events_considered"], 50);
    assert_eq!(parsed["filters"][0]["filter_name"], "git/diff");
    assert_eq!(parsed["filters"][0]["health_score"], 30);
    assert_eq!(parsed["filters"][0]["burst_count"], 3);
    assert_eq!(parsed["filters"][0]["max_burst_size"], 12);
}

#[test]
fn should_disable_color_respects_flag() {
    assert!(should_disable_color(true));
}

#[test]
fn truncate_handles_short_strings() {
    assert_eq!(truncate("foo", 10), "foo");
}

#[test]
fn truncate_adds_ellipsis() {
    let t = truncate("a-very-very-long-filter-name", 10);
    assert!(t.ends_with('…'));
    assert_eq!(t.chars().count(), 10);
}

#[test]
fn score_color_red_for_low_score() {
    let c = Colors::enabled();
    assert_eq!(score_color(20, &c), c.red);
}

#[test]
fn score_color_green_for_high_score() {
    let c = Colors::enabled();
    assert_eq!(score_color(95, &c), c.green);
}

#[test]
fn score_color_yellow_for_mid_score() {
    let c = Colors::enabled();
    assert_eq!(score_color(60, &c), c.yellow);
}
