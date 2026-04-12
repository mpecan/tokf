#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::float_cmp
)]

use super::*;

fn ev(filter: &str, command: &str, ts: &str) -> EventRow {
    EventRow {
        filter_name: filter.to_string(),
        command: command.to_string(),
        timestamp: ts.to_string(),
        output_bytes: 200,
        raw_tokens_est: 50,
        output_tokens_est: 50,
        project: String::new(),
    }
}

fn ev_with_bytes(
    filter: &str,
    command: &str,
    ts: &str,
    output_bytes: i64,
    raw_tokens: i64,
    output_tokens: i64,
) -> EventRow {
    EventRow {
        filter_name: filter.to_string(),
        command: command.to_string(),
        timestamp: ts.to_string(),
        output_bytes,
        raw_tokens_est: raw_tokens,
        output_tokens_est: output_tokens,
        project: String::new(),
    }
}

// ─────────────────────────── parse_iso8601_secs ────────────────────

#[test]
fn iso8601_parses_known_timestamp() {
    // 2024-01-01T00:00:00Z = days from epoch * 86400 = 19723 * 86400
    let secs = parse_iso8601_secs("2024-01-01T00:00:00Z").unwrap();
    assert!((secs - 1_704_067_200.0).abs() < 1.0);
}

#[test]
fn iso8601_rejects_malformed() {
    assert!(parse_iso8601_secs("not a timestamp").is_none());
    assert!(parse_iso8601_secs("2024-01-01T00:00:00").is_none()); // no Z
    assert!(parse_iso8601_secs("2024-01-01 00:00:00Z").is_none()); // space not T
}

#[test]
fn gap_seconds_handles_normal_case() {
    let g = gap_seconds("2024-01-01T00:00:00Z", "2024-01-01T00:00:30Z");
    assert!((g - 30.0).abs() < 1.0);
}

#[test]
fn gap_seconds_returns_inf_on_bad_input() {
    assert_eq!(gap_seconds("bogus", "2024-01-01T00:00:30Z"), f64::INFINITY);
}

// ─────────────────────────── detect_bursts ───────────────────────────

#[test]
fn bursts_flags_exact_match_storm() {
    // 5 identical commands within 10 seconds → burst at threshold 5/window 60
    let events = vec![
        ev("git/diff", "git diff", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:06Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:08Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].filter_name, "git/diff");
    assert_eq!(bursts[0].command, "git diff");
    assert_eq!(bursts[0].burst_size, 5);
}

#[test]
fn bursts_does_not_flag_below_threshold() {
    // Only 4 events → below threshold 5
    let events = vec![
        ev("git/diff", "git diff", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:06Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert!(bursts.is_empty());
}

#[test]
fn bursts_splits_at_window_gap() {
    // Two clusters of 5, separated by a 5-minute gap → two separate
    // bursts, each of size 5
    let events = vec![
        ev("git/diff", "git diff", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:06Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:08Z"),
        // 5-minute gap (300s > 60s window)
        ev("git/diff", "git diff", "2024-01-01T00:05:08Z"),
        ev("git/diff", "git diff", "2024-01-01T00:05:10Z"),
        ev("git/diff", "git diff", "2024-01-01T00:05:12Z"),
        ev("git/diff", "git diff", "2024-01-01T00:05:14Z"),
        ev("git/diff", "git diff", "2024-01-01T00:05:16Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 2);
    assert_eq!(bursts[0].burst_size, 5);
    assert_eq!(bursts[1].burst_size, 5);
}

#[test]
fn bursts_does_not_flag_arg_varying_exploration() {
    // 5 events with different commands in quick succession → exploration,
    // not confusion. Each command has only 1 occurrence so no burst.
    let events = vec![
        ev("git/diff", "git diff foo.rs", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff bar.rs", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff baz.rs", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff qux.rs", "2024-01-01T00:00:06Z"),
        ev("git/diff", "git diff quux.rs", "2024-01-01T00:00:08Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert!(
        bursts.is_empty(),
        "arg-varying exploration must not be flagged: {bursts:?}"
    );
}

#[test]
fn bursts_reports_max_burst_size_correctly() {
    // 7 identical events in a row → one burst of size 7
    let events = vec![
        ev("git/status", "git status", "2024-01-01T00:00:00Z"),
        ev("git/status", "git status", "2024-01-01T00:00:01Z"),
        ev("git/status", "git status", "2024-01-01T00:00:02Z"),
        ev("git/status", "git status", "2024-01-01T00:00:03Z"),
        ev("git/status", "git status", "2024-01-01T00:00:04Z"),
        ev("git/status", "git status", "2024-01-01T00:00:05Z"),
        ev("git/status", "git status", "2024-01-01T00:00:06Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].burst_size, 7);
}

#[test]
fn bursts_handles_empty_input() {
    let bursts = detect_bursts(&[], 5, 60);
    assert!(bursts.is_empty());
}

// ─────────────────────────── detect_workaround_flags ─────────────────

#[test]
fn workaround_counts_no_stat() {
    let events = vec![
        ev("git/diff", "git diff --no-stat", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff --no-stat", "2024-01-01T00:00:01Z"),
        ev(
            "git/diff",
            "git diff --no-stat foo.rs",
            "2024-01-01T00:00:02Z",
        ),
    ];
    let flags = detect_workaround_flags(&events);
    let no_stat = flags.iter().find(|f| f.flag == "--no-stat").unwrap();
    assert_eq!(no_stat.count, 3);
    assert_eq!(no_stat.filter_name, "git/diff");
}

#[test]
fn workaround_counts_short_p_flag() {
    let events = vec![
        ev("git/log", "git log -p", "2024-01-01T00:00:00Z"),
        ev("git/log", "git log -p HEAD~5", "2024-01-01T00:00:01Z"),
    ];
    let flags = detect_workaround_flags(&events);
    let p = flags.iter().find(|f| f.flag == "-p").unwrap();
    assert_eq!(p.count, 2);
}

#[test]
fn workaround_handles_attached_u_value() {
    // `-U10` should match `-U` (git diff context lines)
    let events = vec![ev(
        "git/diff",
        "git diff -U10 foo.rs",
        "2024-01-01T00:00:00Z",
    )];
    let flags = detect_workaround_flags(&events);
    let u = flags.iter().find(|f| f.flag == "-U");
    assert!(u.is_some(), "should detect -U10 as -U variant: {flags:?}");
}

#[test]
fn workaround_handles_format_with_equals() {
    // `--format=oneline` should match `--format`
    let events = vec![ev(
        "git/log",
        "git log --format=oneline",
        "2024-01-01T00:00:00Z",
    )];
    let flags = detect_workaround_flags(&events);
    assert!(flags.iter().any(|f| f.flag == "--format"));
}

#[test]
fn workaround_groups_by_filter() {
    let events = vec![
        ev("git/diff", "git diff -p", "2024-01-01T00:00:00Z"),
        ev("git/log", "git log -p", "2024-01-01T00:00:01Z"),
    ];
    let flags = detect_workaround_flags(&events);
    let diff_p = flags
        .iter()
        .find(|f| f.filter_name == "git/diff" && f.flag == "-p");
    let log_p = flags
        .iter()
        .find(|f| f.filter_name == "git/log" && f.flag == "-p");
    assert!(diff_p.is_some());
    assert!(log_p.is_some());
}

#[test]
fn workaround_empty_input() {
    assert!(detect_workaround_flags(&[]).is_empty());
}

#[test]
fn workaround_no_known_flags() {
    let events = vec![ev("git/status", "git status", "2024-01-01T00:00:00Z")];
    assert!(detect_workaround_flags(&events).is_empty());
}

// ─────────────────────────── detect_empty_retries ────────────────────

#[test]
fn empty_retry_flags_followup() {
    // Empty event followed by another within window → flagged
    let events = vec![
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:00Z", 5, 5, 5),
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:10Z", 200, 50, 50),
    ];
    let retries = detect_empty_retries(&events, 60);
    assert_eq!(retries.len(), 1);
    assert_eq!(retries[0].retry_count, 1);
}

#[test]
fn empty_retry_does_not_flag_solo_empty() {
    // Empty event with no follow-up → not flagged
    let events = vec![ev_with_bytes(
        "git/log",
        "git log",
        "2024-01-01T00:00:00Z",
        5,
        5,
        5,
    )];
    let retries = detect_empty_retries(&events, 60);
    assert!(retries.is_empty());
}

#[test]
fn empty_retry_respects_window() {
    // Empty event followed by event 5 minutes later → outside window
    let events = vec![
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:00Z", 5, 5, 5),
        ev_with_bytes("git/log", "git log", "2024-01-01T00:05:00Z", 200, 50, 50),
    ];
    let retries = detect_empty_retries(&events, 60);
    assert!(retries.is_empty());
}

#[test]
fn empty_retry_does_not_flag_non_empty() {
    let events = vec![
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:00Z", 5000, 200, 200),
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:10Z", 5000, 200, 200),
    ];
    let retries = detect_empty_retries(&events, 60);
    assert!(retries.is_empty());
}

// ─────────────────────────── compute_negative_savings ────────────────

#[test]
fn negative_savings_flags_filters_that_inflate() {
    // raw=10, output=20 → average excess = 10
    let events = vec![
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:00Z", 80, 10, 20),
        ev_with_bytes("git/log", "git log", "2024-01-01T00:00:01Z", 80, 10, 20),
    ];
    let neg = compute_negative_savings(&events);
    assert_eq!(neg.len(), 1);
    assert!((neg[0].avg_excess_tokens - 10.0).abs() < 0.01);
    assert_eq!(neg[0].event_count, 2);
}

#[test]
fn negative_savings_skips_filters_that_save() {
    // raw=100, output=20 → saving 80 → not flagged
    let events = vec![ev_with_bytes(
        "git/diff",
        "git diff",
        "2024-01-01T00:00:00Z",
        80,
        100,
        20,
    )];
    let neg = compute_negative_savings(&events);
    assert!(neg.is_empty());
}

#[test]
fn negative_savings_skips_legacy_zero_raw() {
    // raw_tokens_est = 0 means pre-#raw-tracking legacy event → must skip
    let events = vec![ev_with_bytes(
        "git/old",
        "old cmd",
        "2024-01-01T00:00:00Z",
        80,
        0,
        20,
    )];
    let neg = compute_negative_savings(&events);
    assert!(
        neg.is_empty(),
        "legacy raw=0 events must be excluded: {neg:?}"
    );
}

#[test]
fn negative_savings_empty_input() {
    assert!(compute_negative_savings(&[]).is_empty());
}

// ─────────────────────────── fetch_events (DB) ───────────────────────

fn open_in_memory_db() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE events (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT    NOT NULL,
            command           TEXT    NOT NULL,
            filter_name       TEXT,
            filter_hash       TEXT,
            input_bytes       INTEGER NOT NULL,
            output_bytes      INTEGER NOT NULL,
            input_tokens_est  INTEGER NOT NULL,
            output_tokens_est INTEGER NOT NULL,
            filter_time_ms    INTEGER NOT NULL,
            exit_code         INTEGER NOT NULL,
            pipe_override     INTEGER NOT NULL DEFAULT 0,
            raw_bytes         INTEGER NOT NULL DEFAULT 0,
            raw_tokens_est    INTEGER NOT NULL DEFAULT 0,
            project           TEXT    NOT NULL DEFAULT ''
        );",
    )
    .unwrap();
    conn
}

fn insert_test_event(
    conn: &rusqlite::Connection,
    timestamp: &str,
    command: &str,
    filter_name: Option<&str>,
    output_bytes: i64,
    raw_tokens: i64,
    output_tokens: i64,
    project: &str,
) {
    conn.execute(
        "INSERT INTO events
            (timestamp, command, filter_name, input_bytes, output_bytes,
             input_tokens_est, output_tokens_est,
             raw_bytes, raw_tokens_est, filter_time_ms, exit_code,
             pipe_override, project)
         VALUES
            (?1, ?2, ?3, 100, ?4, ?5, ?6, 100, ?5, 0, 0, 0, ?7)",
        rusqlite::params![
            timestamp,
            command,
            filter_name,
            output_bytes,
            raw_tokens,
            output_tokens,
            project,
        ],
    )
    .unwrap();
}

#[test]
fn fetch_events_filters_null_filter_name() {
    let conn = open_in_memory_db();
    insert_test_event(
        &conn,
        "2024-01-01T00:00:00Z",
        "git status",
        Some("git/status"),
        100,
        25,
        25,
        "",
    );
    insert_test_event(
        &conn,
        "2024-01-01T00:00:01Z",
        "raw command",
        None,
        100,
        25,
        25,
        "",
    );
    let events = fetch_events(&conn, None, true).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].filter_name, "git/status");
}

#[test]
fn fetch_events_excludes_noise_by_default() {
    let conn = open_in_memory_db();
    insert_test_event(
        &conn,
        "2024-01-01T00:00:00Z",
        "git -C /var/folders/abc/.tmpXYZ status",
        Some("git/status"),
        100,
        25,
        25,
        "",
    );
    insert_test_event(
        &conn,
        "2024-01-01T00:00:01Z",
        "git status",
        Some("git/status"),
        100,
        25,
        25,
        "",
    );
    let with_noise = fetch_events(&conn, None, true).unwrap();
    let without_noise = fetch_events(&conn, None, false).unwrap();
    assert_eq!(with_noise.len(), 2);
    assert_eq!(without_noise.len(), 1);
    assert_eq!(without_noise[0].command, "git status");
}

#[test]
fn fetch_events_scopes_to_project() {
    let conn = open_in_memory_db();
    insert_test_event(
        &conn,
        "2024-01-01T00:00:00Z",
        "git status",
        Some("git/status"),
        100,
        25,
        25,
        "/repo/a",
    );
    insert_test_event(
        &conn,
        "2024-01-01T00:00:01Z",
        "git status",
        Some("git/status"),
        100,
        25,
        25,
        "/repo/b",
    );
    insert_test_event(
        &conn,
        "2024-01-01T00:00:02Z",
        "git status",
        Some("git/status"),
        100,
        25,
        25,
        "",
    );
    let scoped = fetch_events(&conn, Some("/repo/a"), true).unwrap();
    // Should match /repo/a + the empty-project legacy row
    assert_eq!(scoped.len(), 2);
    let all = fetch_events(&conn, None, true).unwrap();
    assert_eq!(all.len(), 3);
}
