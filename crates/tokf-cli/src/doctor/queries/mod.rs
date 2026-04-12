//! Doctor signal queries.
//!
//! Strategy: fetch all relevant columns from `events` into memory once,
//! then run signal detectors as **pure functions** over `&[EventRow]`.

use std::collections::HashMap;

use anyhow::Context as _;
use rusqlite::Connection;

use super::noise::{command_shape, is_noise_command};

/// Row pulled from `events`.
#[derive(Debug, Clone)]
pub struct EventRow {
    pub filter_name: String,
    pub command: String,
    pub timestamp: String,
    pub output_bytes: i64,
    pub input_tokens_est: i64,
    pub raw_tokens_est: i64,
    pub output_tokens_est: i64,
    pub filter_time_ms: i64,
    pub exit_code: i32,
    pub pipe_override: bool,
    pub project: String,
}

fn map_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        filter_name: row.get(0)?,
        command: row.get(1)?,
        timestamp: row.get(2)?,
        output_bytes: row.get(3)?,
        input_tokens_est: row.get(4)?,
        raw_tokens_est: row.get(5)?,
        output_tokens_est: row.get(6)?,
        filter_time_ms: row.get(7)?,
        exit_code: row.get(8)?,
        pipe_override: row.get::<_, i64>(9)? != 0,
        project: row.get(10)?,
    })
}

/// Fetch all events with a non-NULL `filter_name`.
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
                    input_tokens_est, raw_tokens_est, output_tokens_est,
                    filter_time_ms, exit_code, pipe_override, project
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

// ─────────────────────── shared session splitter ─────────────────────

/// A contiguous run of events whose consecutive timestamps are all
/// within `window` seconds of each other, and whose total size meets
/// or exceeds `threshold`.
struct Session {
    start: usize,
    end: usize, // exclusive
}

/// Split a time-ordered slice of events into session windows.
///
/// A session breaks when the gap between consecutive events exceeds
/// `window_secs`. Only sessions of `size >= threshold` are returned.
fn split_sessions(events: &[&EventRow], threshold: usize, window_f64: f64) -> Vec<Session> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for i in 1..=events.len() {
        let gap_too_large = i == events.len()
            || gap_seconds(&events[i - 1].timestamp, &events[i].timestamp) > window_f64;
        if gap_too_large {
            if i - start >= threshold {
                out.push(Session { start, end: i });
            }
            start = i;
        }
    }
    out
}

// ─────────────────────────── burst detection ───────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BurstRow {
    pub filter_name: String,
    pub command: String,
    pub burst_size: usize,
    pub failed_count: usize,
    /// Total `filter_time_ms` across all events in the burst.
    pub total_time_ms: i64,
    pub last_seen: String,
}

/// Detect retry-burst sessions using **exact string match** grouping.
pub fn detect_bursts(events: &[EventRow], threshold: usize, window_secs: u64) -> Vec<BurstRow> {
    let mut groups: HashMap<(&str, &str), Vec<&EventRow>> = HashMap::new();
    for ev in events {
        groups
            .entry((&ev.filter_name, &ev.command))
            .or_default()
            .push(ev);
    }
    #[allow(clippy::cast_precision_loss)]
    let window_f64 = window_secs as f64;
    let mut out = Vec::new();
    for ((filter, command), group) in &groups {
        for s in split_sessions(group, threshold, window_f64) {
            let session = &group[s.start..s.end];
            let failed = session.iter().filter(|e| e.exit_code != 0).count();
            let time: i64 = session.iter().map(|e| e.filter_time_ms).sum();
            out.push(BurstRow {
                filter_name: (*filter).to_string(),
                command: (*command).to_string(),
                burst_size: session.len(),
                failed_count: failed,
                total_time_ms: time,
                last_seen: session
                    .last()
                    .map_or_else(String::new, |e| e.timestamp.clone()),
            });
        }
    }
    out.sort_by(|a, b| {
        b.burst_size
            .cmp(&a.burst_size)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

// ─────────────────────── shape-based burst detection ──────────────────

/// A burst session grouped by **command shape** (program + subcommand).
///
/// Captures the pattern where the agent cycles through flag variants of
/// the same command (`git diff`, `git diff --name-only`, `git diff
/// --stat`) trying to escape a filter.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ShapeBurstRow {
    pub filter_name: String,
    pub shape: String,
    pub burst_size: usize,
    pub distinct_commands: usize,
    /// `distinct_commands / burst_size` — 0.0–1.0.
    pub arg_uniqueness: f64,
    pub failed_count: usize,
    pub last_seen: String,
}

/// Detect shape-based burst sessions.
pub fn detect_shape_bursts(
    events: &[EventRow],
    threshold: usize,
    window_secs: u64,
) -> Vec<ShapeBurstRow> {
    use std::collections::HashSet;
    // Pre-compute shapes with dedup: same command → same shape.
    let mut shape_cache: HashMap<&str, String> = HashMap::new();
    for ev in events {
        shape_cache
            .entry(&ev.command)
            .or_insert_with(|| command_shape(&ev.command));
    }
    let mut groups: HashMap<(&str, &str), Vec<&EventRow>> = HashMap::new();
    for ev in events {
        let Some(shape) = shape_cache.get(ev.command.as_str()) else {
            continue; // shouldn't happen — pre-computed above
        };
        groups
            .entry((&ev.filter_name, shape.as_str()))
            .or_default()
            .push(ev);
    }
    #[allow(clippy::cast_precision_loss)]
    let window_f64 = window_secs as f64;
    let mut out = Vec::new();
    for ((filter, shape), group) in &groups {
        for s in split_sessions(group, threshold, window_f64) {
            let session = &group[s.start..s.end];
            let distinct: HashSet<&str> = session.iter().map(|e| e.command.as_str()).collect();
            let failed = session.iter().filter(|e| e.exit_code != 0).count();
            #[allow(clippy::cast_precision_loss)]
            let uniqueness = distinct.len() as f64 / session.len().max(1) as f64;
            out.push(ShapeBurstRow {
                filter_name: (*filter).to_string(),
                shape: (*shape).to_string(),
                burst_size: session.len(),
                distinct_commands: distinct.len(),
                arg_uniqueness: uniqueness,
                failed_count: failed,
                last_seen: session
                    .last()
                    .map_or_else(String::new, |e| e.timestamp.clone()),
            });
        }
    }
    out.sort_by(|a, b| {
        b.burst_size
            .cmp(&a.burst_size)
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

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
/// Returns seconds since the Unix epoch as `f64`.
fn parse_iso8601_secs(ts: &str) -> Option<f64> {
    if ts.len() != 20 || ts.as_bytes()[10] != b'T' || ts.as_bytes()[19] != b'Z' {
        return None;
    }
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: i64 = ts.get(11..13)?.parse().ok()?;
    let min: i64 = ts.get(14..16)?.parse().ok()?;
    let sec: i64 = ts.get(17..19)?.parse().ok()?;
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
    "-U",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkaroundFlagRow {
    pub filter_name: String,
    pub flag: String,
    pub count: usize,
}

/// Check if `token` matches a workaround flag (exact, `=`-prefixed, or
/// `-U` numeric form). Avoids per-iteration `format!` allocation.
fn token_matches_flag(token: &str, flag: &str) -> bool {
    if token == flag {
        return true;
    }
    // `--format=oneline` style: flag followed by `=`
    if token.len() > flag.len() && token.starts_with(flag) && token.as_bytes()[flag.len()] == b'=' {
        return true;
    }
    // `-U10` style: `-U` prefix with attached numeric
    flag == "-U" && token.starts_with("-U") && token.len() > 2
}

pub fn detect_workaround_flags(events: &[EventRow]) -> Vec<WorkaroundFlagRow> {
    let mut counts: HashMap<(&str, &'static str), usize> = HashMap::new();
    for ev in events {
        for token in ev.command.split_whitespace() {
            for &flag in WORKAROUND_FLAGS {
                if token_matches_flag(token, flag) {
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

// ─────────────────────────── empty-output chains ────────────────────────

/// An empty-output chain for the same command.
///
/// Consecutive events where each has output below the empty threshold,
/// all within `window` of each other. A chain of 5 empties in a row is
/// much more alarming than 5 isolated empties.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EmptyChainRow {
    pub filter_name: String,
    pub command: String,
    pub chain_count: usize,
    pub max_chain_length: usize,
    pub total_empty_events: usize,
}

const EMPTY_OUTPUT_THRESHOLD_BYTES: i64 = 50;

pub fn detect_empty_chains(events: &[EventRow], window_secs: u64) -> Vec<EmptyChainRow> {
    let mut by_command: HashMap<(&str, &str), Vec<&EventRow>> = HashMap::new();
    for ev in events {
        by_command
            .entry((&ev.filter_name, &ev.command))
            .or_default()
            .push(ev);
    }
    #[allow(clippy::cast_precision_loss)]
    let window_f64 = window_secs as f64;
    let mut out = Vec::new();
    for ((filter, command), group) in by_command {
        let mut chain_count = 0usize;
        let mut max_chain = 0usize;
        let mut total_empty = 0usize;
        let mut current_chain = 0usize;
        let mut chain_start_ts: Option<&str> = None;
        for ev in &group {
            let is_empty = ev.output_bytes < EMPTY_OUTPUT_THRESHOLD_BYTES;
            let within_window =
                chain_start_ts.is_none_or(|start| gap_seconds(start, &ev.timestamp) <= window_f64);
            if is_empty && within_window {
                if current_chain == 0 {
                    chain_start_ts = Some(&ev.timestamp);
                }
                current_chain += 1;
            } else {
                if current_chain >= 2 {
                    chain_count += 1;
                    max_chain = max_chain.max(current_chain);
                    total_empty += current_chain;
                }
                current_chain = usize::from(is_empty);
                chain_start_ts = if is_empty { Some(&ev.timestamp) } else { None };
            }
        }
        if current_chain >= 2 {
            chain_count += 1;
            max_chain = max_chain.max(current_chain);
            total_empty += current_chain;
        }
        if chain_count > 0 {
            out.push(EmptyChainRow {
                filter_name: filter.to_string(),
                command: command.to_string(),
                chain_count,
                max_chain_length: max_chain,
                total_empty_events: total_empty,
            });
        }
    }
    out.sort_by(|a, b| {
        b.max_chain_length
            .cmp(&a.max_chain_length)
            .then_with(|| b.total_empty_events.cmp(&a.total_empty_events))
            .then_with(|| a.filter_name.cmp(&b.filter_name))
    });
    out
}

// ─────────────────────────── negative savings ───────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NegativeSavingsRow {
    pub filter_name: String,
    pub avg_excess_tokens: f64,
    pub event_count: usize,
}

pub fn compute_negative_savings(events: &[EventRow]) -> Vec<NegativeSavingsRow> {
    let mut acc: HashMap<&str, (f64, usize)> = HashMap::new();
    for ev in events {
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
                return None;
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

// ─────────────────────────── per-filter aggregates ──────────────────────

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FilterStats {
    pub event_count: usize,
    pub failed_count: usize,
    pub pipe_override_count: usize,
    pub total_filter_time_ms: i64,
}

pub fn compute_filter_stats(
    events: &[EventRow],
) -> std::collections::BTreeMap<String, FilterStats> {
    let mut stats = std::collections::BTreeMap::new();
    for ev in events {
        let entry = stats
            .entry(ev.filter_name.clone())
            .or_insert_with(FilterStats::default);
        entry.event_count += 1;
        if ev.exit_code != 0 {
            entry.failed_count += 1;
        }
        if ev.pipe_override {
            entry.pipe_override_count += 1;
        }
        entry.total_filter_time_ms += ev.filter_time_ms;
    }
    stats
}

#[cfg(test)]
mod tests;
