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
        input_tokens_est: 50,
        raw_tokens_est: 50,
        output_tokens_est: 50,
        filter_time_ms: 10,
        exit_code: 0,
        pipe_override: false,
        project: String::new(),
    }
}

fn ev_full(
    filter: &str,
    command: &str,
    ts: &str,
    output_bytes: i64,
    raw_tokens: i64,
    output_tokens: i64,
    exit_code: i32,
    filter_time_ms: i64,
) -> EventRow {
    EventRow {
        filter_name: filter.to_string(),
        command: command.to_string(),
        timestamp: ts.to_string(),
        output_bytes,
        input_tokens_est: raw_tokens,
        raw_tokens_est: raw_tokens,
        output_tokens_est: output_tokens,
        filter_time_ms,
        exit_code,
        pipe_override: false,
        project: String::new(),
    }
}

// ─────────────────────────── parse_iso8601_secs ────────────────────

#[test]
fn iso8601_parses_known_timestamp() {
    let secs = parse_iso8601_secs("2024-01-01T00:00:00Z").unwrap();
    assert!((secs - 1_704_067_200.0).abs() < 1.0);
}

#[test]
fn iso8601_rejects_malformed() {
    assert!(parse_iso8601_secs("not a timestamp").is_none());
    assert!(parse_iso8601_secs("2024-01-01T00:00:00").is_none());
    assert!(parse_iso8601_secs("2024-01-01 00:00:00Z").is_none());
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
    let events = vec![
        ev("git/diff", "git diff", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:06Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:08Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].burst_size, 5);
}

#[test]
fn bursts_tracks_failures() {
    let events = vec![
        ev_full("f", "cmd", "2024-01-01T00:00:00Z", 200, 50, 50, 1, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:02Z", 200, 50, 50, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:04Z", 200, 50, 50, 1, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:06Z", 200, 50, 50, 1, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:08Z", 200, 50, 50, 0, 10),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts[0].failed_count, 3);
    assert_eq!(bursts[0].total_time_ms, 50);
}

#[test]
fn bursts_does_not_flag_below_threshold() {
    let events = vec![
        ev("git/diff", "git diff", "2024-01-01T00:00:00Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:02Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:04Z"),
        ev("git/diff", "git diff", "2024-01-01T00:00:06Z"),
    ];
    assert!(detect_bursts(&events, 5, 60).is_empty());
}

#[test]
fn bursts_splits_at_window_gap() {
    let events = vec![
        ev("f", "cmd", "2024-01-01T00:00:00Z"),
        ev("f", "cmd", "2024-01-01T00:00:02Z"),
        ev("f", "cmd", "2024-01-01T00:00:04Z"),
        ev("f", "cmd", "2024-01-01T00:00:06Z"),
        ev("f", "cmd", "2024-01-01T00:00:08Z"),
        ev("f", "cmd", "2024-01-01T00:05:08Z"),
        ev("f", "cmd", "2024-01-01T00:05:10Z"),
        ev("f", "cmd", "2024-01-01T00:05:12Z"),
        ev("f", "cmd", "2024-01-01T00:05:14Z"),
        ev("f", "cmd", "2024-01-01T00:05:16Z"),
    ];
    let bursts = detect_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 2);
}

#[test]
fn bursts_does_not_flag_arg_varying_exploration() {
    let events = vec![
        ev("f", "git diff foo.rs", "2024-01-01T00:00:00Z"),
        ev("f", "git diff bar.rs", "2024-01-01T00:00:02Z"),
        ev("f", "git diff baz.rs", "2024-01-01T00:00:04Z"),
        ev("f", "git diff qux.rs", "2024-01-01T00:00:06Z"),
        ev("f", "git diff quux.rs", "2024-01-01T00:00:08Z"),
    ];
    assert!(detect_bursts(&events, 5, 60).is_empty());
}

#[test]
fn bursts_handles_empty_input() {
    assert!(detect_bursts(&[], 5, 60).is_empty());
}

// ─────────────────────── detect_shape_bursts ─────────────────────────

#[test]
fn shape_bursts_catches_flag_cycling() {
    let events = vec![
        // All have shape "git diff <args>" (each has ≥1 arg after subcommand)
        ev("git diff", "git diff --stat", "2024-01-01T00:00:00Z"),
        ev("git diff", "git diff --name-only", "2024-01-01T00:00:02Z"),
        ev("git diff", "git diff -p", "2024-01-01T00:00:04Z"),
        ev("git diff", "git diff --no-stat", "2024-01-01T00:00:06Z"),
        ev("git diff", "git diff --raw", "2024-01-01T00:00:08Z"),
    ];
    let bursts = detect_shape_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 1, "shape burst should fire: {bursts:?}");
    assert_eq!(bursts[0].burst_size, 5);
    assert_eq!(bursts[0].distinct_commands, 5);
    assert!((bursts[0].arg_uniqueness - 1.0).abs() < 0.01);
}

#[test]
fn shape_bursts_low_uniqueness_for_exact_repeats() {
    let events = vec![
        ev("f", "git diff --stat", "2024-01-01T00:00:00Z"),
        ev("f", "git diff --stat", "2024-01-01T00:00:02Z"),
        ev("f", "git diff --stat", "2024-01-01T00:00:04Z"),
        ev("f", "git diff --stat", "2024-01-01T00:00:06Z"),
        ev("f", "git diff --stat", "2024-01-01T00:00:08Z"),
    ];
    let bursts = detect_shape_bursts(&events, 5, 60);
    assert_eq!(bursts.len(), 1);
    // All 5 are the same command → 1 distinct / 5 = 0.2
    assert!((bursts[0].arg_uniqueness - 0.2).abs() < 0.01);
}

#[test]
fn shape_bursts_empty_input() {
    assert!(detect_shape_bursts(&[], 5, 60).is_empty());
}

// ─────────────────────────── workaround flags ────────────────────────

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
}

#[test]
fn workaround_handles_attached_u_value() {
    let events = vec![ev(
        "git/diff",
        "git diff -U10 foo.rs",
        "2024-01-01T00:00:00Z",
    )];
    let flags = detect_workaround_flags(&events);
    assert!(flags.iter().any(|f| f.flag == "-U"));
}

#[test]
fn workaround_handles_format_with_equals() {
    let events = vec![ev(
        "git/log",
        "git log --format=oneline",
        "2024-01-01T00:00:00Z",
    )];
    let flags = detect_workaround_flags(&events);
    assert!(flags.iter().any(|f| f.flag == "--format"));
}

#[test]
fn workaround_empty_input() {
    assert!(detect_workaround_flags(&[]).is_empty());
}

// ─────────────────────────── detect_empty_chains ─────────────────────

#[test]
fn empty_chain_detects_consecutive_empties() {
    let events = vec![
        ev_full("f", "cmd", "2024-01-01T00:00:00Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:02Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:04Z", 5, 5, 5, 0, 10),
    ];
    let chains = detect_empty_chains(&events, 60);
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].max_chain_length, 3);
    assert_eq!(chains[0].chain_count, 1);
    assert_eq!(chains[0].total_empty_events, 3);
}

#[test]
fn empty_chain_breaks_on_non_empty() {
    let events = vec![
        ev_full("f", "cmd", "2024-01-01T00:00:00Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:02Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:04Z", 500, 50, 50, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:06Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:08Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:10Z", 5, 5, 5, 0, 10),
    ];
    let chains = detect_empty_chains(&events, 60);
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].chain_count, 2, "should have 2 chains");
    assert_eq!(chains[0].max_chain_length, 3, "longest chain = 3");
    assert_eq!(chains[0].total_empty_events, 5);
}

#[test]
fn empty_chain_ignores_single_empty() {
    let events = vec![ev_full("f", "cmd", "2024-01-01T00:00:00Z", 5, 5, 5, 0, 10)];
    assert!(detect_empty_chains(&events, 60).is_empty());
}

#[test]
fn empty_chain_respects_window() {
    let events = vec![
        ev_full("f", "cmd", "2024-01-01T00:00:00Z", 5, 5, 5, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:05:00Z", 5, 5, 5, 0, 10),
    ];
    assert!(detect_empty_chains(&events, 60).is_empty());
}

#[test]
fn empty_chain_empty_input() {
    assert!(detect_empty_chains(&[], 60).is_empty());
}

// ─────────────────────────── negative savings ────────────────────────

#[test]
fn negative_savings_flags_filters_that_inflate() {
    let events = vec![
        ev_full("f", "cmd", "2024-01-01T00:00:00Z", 80, 10, 20, 0, 10),
        ev_full("f", "cmd", "2024-01-01T00:00:01Z", 80, 10, 20, 0, 10),
    ];
    let neg = compute_negative_savings(&events);
    assert_eq!(neg.len(), 1);
    assert!((neg[0].avg_excess_tokens - 10.0).abs() < 0.01);
}

#[test]
fn negative_savings_skips_saving_filters() {
    let events = vec![ev_full(
        "f",
        "cmd",
        "2024-01-01T00:00:00Z",
        80,
        100,
        20,
        0,
        10,
    )];
    assert!(compute_negative_savings(&events).is_empty());
}

#[test]
fn negative_savings_skips_legacy_zero_raw() {
    let events = vec![ev_full(
        "f",
        "cmd",
        "2024-01-01T00:00:00Z",
        80,
        0,
        20,
        0,
        10,
    )];
    assert!(compute_negative_savings(&events).is_empty());
}

// ─────────────────────────── compute_filter_stats ────────────────────

#[test]
fn filter_stats_counts_failures_and_overrides() {
    let events = vec![
        {
            let mut e = ev("f", "cmd", "2024-01-01T00:00:00Z");
            e.exit_code = 1;
            e
        },
        {
            let mut e = ev("f", "cmd", "2024-01-01T00:00:01Z");
            e.pipe_override = true;
            e
        },
        ev("f", "cmd", "2024-01-01T00:00:02Z"),
    ];
    let stats = compute_filter_stats(&events);
    let s = stats.get("f").unwrap();
    assert_eq!(s.event_count, 3);
    assert_eq!(s.failed_count, 1);
    assert_eq!(s.pipe_override_count, 1);
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
    assert_eq!(scoped.len(), 2);
    let all = fetch_events(&conn, None, true).unwrap();
    assert_eq!(all.len(), 3);
}
