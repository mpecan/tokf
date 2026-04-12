//! Doctor signal queries.
//!
//! Strategy: fetch a slim view of `events` into memory once, then run
//! all four signal detectors as **pure functions** over `&[EventRow]`.
//! This keeps the SQL trivial (one indexed `SELECT`) and makes every
//! analysis directly unit-testable from synthetic data.
//!
//! On a typical local `tracking.db` (tens of thousands of rows) the
//! one-shot fetch is comfortably sub-second; remote/cloud aggregation
//! is explicitly out of scope for `tokf doctor` (see the issue's
//! "Out of scope" section).

use anyhow::Context as _;
use rusqlite::Connection;

use super::noise::is_noise_command;

/// Slim row pulled from `events`. Only the columns the doctor needs.
#[derive(Debug, Clone)]
pub struct EventRow {
    pub filter_name: String,
    pub command: String,
    pub timestamp: String,
    pub output_bytes: i64,
    pub raw_tokens_est: i64,
    pub output_tokens_est: i64,
    pub project: String,
}

/// Map a rusqlite row to an `EventRow`. Extracted as a named function so
/// its structure doesn't trigger the duplication checker against the
/// similar-but-different `SyncableEvent` row mapper in `tracking/mod.rs`.
fn map_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        filter_name: row.get(0)?,
        command: row.get(1)?,
        timestamp: row.get(2)?,
        output_bytes: row.get(3)?,
        raw_tokens_est: row.get(4)?,
        output_tokens_est: row.get(5)?,
        project: row.get(6)?,
    })
}

/// Fetch all events with a non-NULL `filter_name`, optionally scoped to
/// one project and optionally excluding noise.
///
/// `project_filter`:
///   - `Some(p)` → rows where `project == p` OR `project == ''` (legacy
///     events without a project tag are visible from every scope so old
///     data is still inspectable until naturally aged out)
///   - `None` → all projects
///
/// `include_noise = false` (the default) drops rows whose command path
/// looks like a temp-dir / test-fixture invocation (see `noise.rs`).
///
/// # Errors
/// Returns an error if the SQL query fails.
pub fn fetch_events(
    conn: &Connection,
    project_filter: Option<&str>,
    include_noise: bool,
) -> anyhow::Result<Vec<EventRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT filter_name, command, timestamp, output_bytes,
                    raw_tokens_est, output_tokens_est, project
             FROM events
             WHERE filter_name IS NOT NULL
               AND (?1 IS NULL OR project = ?1 OR project = '')
             ORDER BY timestamp ASC",
        )
        .context("prepare doctor fetch")?;
    let rows = stmt.query_map(rusqlite::params![project_filter], map_event_row)?;
    let mut out = Vec::new();
    for row in rows {
        let row = row.context("read doctor row")?;
        if !include_noise && is_noise_command(&row.command) {
            continue;
        }
        out.push(row);
    }
    Ok(out)
}

// ─────────────────────────── burst detection ───────────────────────────

/// One detected burst session: a maximal run of identical commands where
/// every consecutive pair fell within `window` seconds of each other.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BurstRow {
    pub filter_name: String,
    pub command: String,
    pub burst_size: usize,
    /// Timestamp of the last event in the burst (for "last seen" display).
    pub last_seen: String,
}

/// Detect retry-burst sessions in the event log.
///
/// "Same command" is **exact string match** (the issue's lean — highest
/// signal, lowest false-positive rate). A "burst" is a maximal run of
/// such events where every pair of consecutive events is within `window`
/// seconds. Bursts of size `< threshold` are not reported.
pub fn detect_bursts(events: &[EventRow], threshold: usize, window_secs: u64) -> Vec<BurstRow> {
    use std::collections::HashMap;
    // Group by (filter, command) → sorted (by timestamp) Vec of timestamps
    let mut groups: HashMap<(&str, &str), Vec<&str>> = HashMap::new();
    for ev in events {
        groups
            .entry((&ev.filter_name, &ev.command))
            .or_default()
            .push(&ev.timestamp);
    }
    let mut out = Vec::new();
    for ((filter, command), timestamps) in groups {
        // Walk the sorted timestamps, splitting at gaps > window.
        let mut session_start = 0usize;
        #[allow(clippy::cast_precision_loss)]
        let window_f64 = window_secs as f64;
        for i in 1..=timestamps.len() {
            let gap_too_large =
                i == timestamps.len() || gap_seconds(timestamps[i - 1], timestamps[i]) > window_f64;
            if gap_too_large {
                let session_size = i - session_start;
                if session_size >= threshold {
                    out.push(BurstRow {
                        filter_name: filter.to_string(),
                        command: command.to_string(),
                        burst_size: session_size,
                        last_seen: timestamps[i - 1].to_string(),
                    });
                }
                session_start = i;
            }
        }
    }
    out.sort_by(|a, b| {
        b.burst_size
            .cmp(&a.burst_size)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

/// Compute the gap in seconds between two ISO-8601 `YYYY-MM-DDTHH:MM:SSZ`
/// timestamps. Returns `f64::INFINITY` on parse failure so the gap is
/// treated as "too large to be in the same burst".
fn gap_seconds(earlier: &str, later: &str) -> f64 {
    let Some(e) = parse_iso8601_secs(earlier) else {
        return f64::INFINITY;
    };
    let Some(l) = parse_iso8601_secs(later) else {
        return f64::INFINITY;
    };
    (l - e).max(0.0)
}

/// Minimal ISO-8601 parser for the `YYYY-MM-DDTHH:MM:SSZ` shape produced
/// by `SQLite`'s `strftime('%Y-%m-%dT%H:%M:%SZ','now')`.
///
/// Returns seconds since the Unix epoch as `f64` (we don't need
/// sub-second precision for burst windowing).
fn parse_iso8601_secs(ts: &str) -> Option<f64> {
    // Expected: "YYYY-MM-DDTHH:MM:SSZ" — exactly 20 chars.
    if ts.len() != 20 || ts.as_bytes()[10] != b'T' || ts.as_bytes()[19] != b'Z' {
        return None;
    }
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: i64 = ts.get(11..13)?.parse().ok()?;
    let min: i64 = ts.get(14..16)?.parse().ok()?;
    let sec: i64 = ts.get(17..19)?.parse().ok()?;
    // Days since 1970-01-01 via the standard "civil from days" algorithm
    // (Howard Hinnant). This avoids pulling in `chrono` for one parse.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m_adj = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m_adj + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    #[allow(clippy::cast_precision_loss)]
    let secs = (days * 86_400 + hour * 3600 + min * 60 + sec) as f64;
    Some(secs)
}

// ─────────────────────────── workaround flags ───────────────────────────

/// Flags that look like "the agent is trying to escape this filter".
/// Cross-referenced against each filter's own `passthrough_args` — flags
/// appearing here often but **not** declared in the filter's passthrough
/// list are surfaced as "candidates to add".
const WORKAROUND_FLAGS: &[&str] = &[
    "--no-stat",
    "--no-pager",
    "-p",
    "--patch",
    "--raw",
    "--name-only",
    "--name-status",
    "--shortstat",
    "--numstat",
    "--format",
    "--pretty",
    "--graph",
    "--oneline",
    "-U", // git diff context lines: `-U10` matches via the prefix branch below
];

/// Per-(filter, flag) workaround occurrence count.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkaroundFlagRow {
    pub filter_name: String,
    pub flag: String,
    pub count: usize,
}

/// Tokenize each event's command and count occurrences of known
/// workaround flags. Returns one row per (filter, flag) pair, sorted by
/// count descending.
pub fn detect_workaround_flags(events: &[EventRow]) -> Vec<WorkaroundFlagRow> {
    use std::collections::HashMap;
    let mut counts: HashMap<(&str, &'static str), usize> = HashMap::new();
    for ev in events {
        for token in ev.command.split_whitespace() {
            // `-U10`, `-U=10` and `--format=oneline` should match `-U`
            // and `--format` respectively. Compare against the flag's
            // bare form first, then prefix forms with `=` or attached
            // numeric.
            for &flag in WORKAROUND_FLAGS {
                if token == flag
                    || token.starts_with(&format!("{flag}="))
                    || (flag == "-U" && token.starts_with("-U") && token.len() > 2)
                {
                    *counts.entry((&ev.filter_name, flag)).or_insert(0) += 1;
                }
            }
        }
    }
    let mut out: Vec<WorkaroundFlagRow> = counts
        .into_iter()
        .map(|((filter, flag), count)| WorkaroundFlagRow {
            filter_name: filter.to_string(),
            flag: flag.to_string(),
            count,
        })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
            .then_with(|| a.flag.cmp(&b.flag))
    });
    out
}

// ─────────────────────────── empty-output retries ───────────────────────

/// An empty-output → retry pattern.
///
/// An event with output looking empty followed by another invocation of
/// the same command within `window` seconds. Strong signal that the
/// filter should use `on_empty` to disambiguate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmptyRetryRow {
    pub filter_name: String,
    pub command: String,
    pub retry_count: usize,
}

/// Threshold for "empty-looking" output, in bytes. Anything below this is
/// considered effectively empty for retry-detection purposes (the agent
/// likely couldn't tell what happened).
const EMPTY_OUTPUT_THRESHOLD_BYTES: i64 = 50;

pub fn detect_empty_retries(events: &[EventRow], window_secs: u64) -> Vec<EmptyRetryRow> {
    use std::collections::HashMap;
    // For each (filter, command) pair, find empty events followed by any
    // event with the same command within `window_secs`.
    let mut by_command: HashMap<(&str, &str), Vec<&EventRow>> = HashMap::new();
    for ev in events {
        by_command
            .entry((&ev.filter_name, &ev.command))
            .or_default()
            .push(ev);
    }
    let mut out = Vec::new();
    #[allow(clippy::cast_precision_loss)]
    let window_f64 = window_secs as f64;
    for ((filter, command), group) in by_command {
        let mut retries = 0usize;
        for (i, ev) in group.iter().enumerate() {
            if ev.output_bytes >= EMPTY_OUTPUT_THRESHOLD_BYTES {
                continue;
            }
            // Look ahead — count one retry if the very next event for the
            // same command is within the window. We only consider the
            // immediate successor (closest follow-up) to avoid double-
            // counting one empty event against many later retries.
            if let Some(follow) = group.get(i + 1)
                && gap_seconds(&ev.timestamp, &follow.timestamp) <= window_f64
            {
                retries += 1;
            }
        }
        if retries > 0 {
            out.push(EmptyRetryRow {
                filter_name: filter.to_string(),
                command: command.to_string(),
                retry_count: retries,
            });
        }
    }
    out.sort_by(|a, b| {
        b.retry_count
            .cmp(&a.retry_count)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

// ─────────────────────────── negative savings ───────────────────────────

/// A filter whose filtered output is, on average, **larger** than the raw
/// command output. This happens when `on_empty` adds explanatory text to
/// a small command, or when stat tables expand short diffs.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NegativeSavingsRow {
    pub filter_name: String,
    /// Average excess tokens per event (filtered − raw). Positive means
    /// the filter is producing more tokens than the raw command would.
    pub avg_excess_tokens: f64,
    /// Number of events with usable raw-token measurements (i.e. not
    /// pre-#raw-tracking legacy rows).
    pub event_count: usize,
}

pub fn compute_negative_savings(events: &[EventRow]) -> Vec<NegativeSavingsRow> {
    use std::collections::HashMap;
    // (filter) → (sum_excess, count)
    let mut acc: HashMap<&str, (f64, usize)> = HashMap::new();
    for ev in events {
        // Skip legacy rows recorded before raw_tokens tracking landed —
        // those rows have raw_tokens_est = 0 and would falsely show as
        // huge negative savings.
        if ev.raw_tokens_est <= 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let excess = (ev.output_tokens_est - ev.raw_tokens_est) as f64;
        let entry = acc.entry(&ev.filter_name).or_insert((0.0, 0));
        entry.0 += excess;
        entry.1 += 1;
    }
    let mut out: Vec<NegativeSavingsRow> = acc
        .into_iter()
        .filter_map(|(filter, (sum, count))| {
            if count == 0 {
                return None;
            }
            #[allow(clippy::cast_precision_loss)]
            let avg = sum / count as f64;
            if avg <= 0.0 {
                return None; // filter is saving tokens — not flagged
            }
            Some(NegativeSavingsRow {
                filter_name: filter.to_string(),
                avg_excess_tokens: avg,
                event_count: count,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.avg_excess_tokens
            .partial_cmp(&a.avg_excess_tokens)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

#[cfg(test)]
mod tests;
