//! `tokf doctor` — post-hoc analysis of `tracking.db` to surface filters
//! that may be causing agent confusion.
//!
//! See `docs/diagnostics.md` for end-user documentation. Orchestration
//! lives here; SQL/analysis lives in `queries`, rendering in `render`,
//! noise/shape helpers in `noise`.

pub mod noise;
pub mod queries;
pub mod render;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context as _;
use rusqlite::Connection;

use crate::config::ResolvedFilter;

use self::queries::{
    BurstRow, EmptyRetryRow, EventRow, NegativeSavingsRow, WorkaroundFlagRow,
    compute_negative_savings, detect_bursts, detect_empty_retries, detect_workaround_flags,
    fetch_events,
};

/// Options controlling a single `tokf doctor` invocation.
#[derive(Debug, Clone)]
pub struct DoctorOpts<'a> {
    pub burst_threshold: usize,
    pub window_secs: u64,
    pub project_filter: Option<&'a str>,
    pub include_noise: bool,
    pub filter_filter: Option<&'a str>,
    pub sort_by: SortBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// Composite health score ascending (worst first). Default.
    #[default]
    Health,
    /// Total burst event count descending.
    Bursts,
    /// Total token impact descending.
    Tokens,
}

/// One per-filter row in the doctor's report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FilterReport {
    pub filter_name: String,
    pub event_count: usize,
    /// Number of distinct burst sessions for this filter.
    pub burst_count: usize,
    /// Largest single burst seen for this filter.
    pub max_burst_size: usize,
    /// Workaround flags this filter received that are NOT in its
    /// `passthrough_args`. Empty if cross-reference data isn't available.
    pub untracked_workaround_flags: Vec<WorkaroundFlagSuggestion>,
    /// Empty-output → retry pattern count for this filter.
    pub empty_retry_count: usize,
    /// Average excess tokens (positive = filter outputs more than raw).
    /// `None` when the filter has no usable raw-token measurements.
    pub avg_excess_tokens: Option<f64>,
    /// Composite health score 0–100 (lower is worse).
    pub health_score: u8,
}

/// A workaround flag the agent passes that the filter doesn't declare in
/// its `passthrough_args`. The doctor surfaces these as "consider adding
/// to the filter config" suggestions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkaroundFlagSuggestion {
    pub flag: String,
    pub count: usize,
}

/// Top-level doctor report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DoctorReport {
    pub total_events_considered: usize,
    pub project_filter: Option<String>,
    pub include_noise: bool,
    pub burst_threshold: usize,
    pub window_secs: u64,
    pub filters: Vec<FilterReport>,
    /// Detailed burst breakdown — useful for the JSON output.
    pub bursts: Vec<BurstRow>,
    pub empty_retries: Vec<EmptyRetryRow>,
    pub negative_savings: Vec<NegativeSavingsRow>,
    pub workaround_flags: Vec<WorkaroundFlagRow>,
}

/// Run the doctor analysis end-to-end.
///
/// `passthrough_args_by_filter` lets the caller cross-reference workaround
/// flags against each filter's declared `passthrough_args`. Pass an empty
/// map to disable cross-referencing — every workaround flag will then be
/// reported as untracked. The CLI populates this from
/// `crate::resolve::discover_filters`.
///
/// # Errors
/// Returns an error if the SQL fetch fails.
pub fn run(
    conn: &Connection,
    opts: &DoctorOpts<'_>,
    filters: &[ResolvedFilter],
) -> anyhow::Result<DoctorReport> {
    let events = fetch_events(conn, opts.project_filter, opts.include_noise)
        .context("doctor: fetch events")?;
    let total_events_considered = events.len();

    let mut bursts = detect_bursts(&events, opts.burst_threshold, opts.window_secs);
    let mut workaround_flags = detect_workaround_flags(&events);
    let mut empty_retries = detect_empty_retries(&events, opts.window_secs);
    let mut negative_savings = compute_negative_savings(&events);

    // If `--filter <name>` is set, scope every section to that one filter
    // so the burst-detail / suggestions / negative-savings blocks don't
    // surface unrelated noise.
    if let Some(only) = opts.filter_filter {
        bursts.retain(|b| b.filter_name == only);
        workaround_flags.retain(|w| w.filter_name == only);
        empty_retries.retain(|r| r.filter_name == only);
        negative_savings.retain(|n| n.filter_name == only);
    }

    // Cross-reference workaround flags with each filter's passthrough_args.
    let passthrough_lookup = build_passthrough_lookup(filters);

    let filter_reports = build_filter_reports(
        &events,
        &bursts,
        &workaround_flags,
        &empty_retries,
        &negative_savings,
        &passthrough_lookup,
        opts.filter_filter,
        opts.sort_by,
    );

    Ok(DoctorReport {
        total_events_considered,
        project_filter: opts.project_filter.map(ToString::to_string),
        include_noise: opts.include_noise,
        burst_threshold: opts.burst_threshold,
        window_secs: opts.window_secs,
        filters: filter_reports,
        bursts,
        workaround_flags,
        empty_retries,
        negative_savings,
    })
}

/// Build a lookup `filter_name → set of passthrough_args` so the report
/// can mark workaround flags as "already declared" vs "candidate to add".
///
/// The key uses the **first command pattern** of each filter (e.g.
/// `"git diff"`), because that's what `commands.rs` stores in
/// `events.filter_name` when it records a tracking event. The relative
/// path form (`"git/diff"`) is *not* what's in the DB.
///
/// We additionally insert each filter under its slash-form name as an
/// alias, so `--filter git/diff` works as a user-friendly shortcut even
/// though the DB stores `"git diff"`.
fn build_passthrough_lookup(filters: &[ResolvedFilter]) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in filters {
        let pattern = f.config.command.first().to_string();
        if pattern.is_empty() {
            continue;
        }
        let args: BTreeSet<String> = f.config.passthrough_args.iter().cloned().collect();
        out.insert(pattern, args);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn build_filter_reports(
    events: &[EventRow],
    bursts: &[BurstRow],
    workaround_flags: &[WorkaroundFlagRow],
    empty_retries: &[EmptyRetryRow],
    negative_savings: &[NegativeSavingsRow],
    passthrough_lookup: &BTreeMap<String, BTreeSet<String>>,
    filter_filter: Option<&str>,
    sort_by: SortBy,
) -> Vec<FilterReport> {
    let mut event_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut filter_names: BTreeSet<String> = BTreeSet::new();
    for ev in events {
        *event_counts.entry(ev.filter_name.clone()).or_insert(0) += 1;
        filter_names.insert(ev.filter_name.clone());
    }

    let signals = SignalSlices {
        bursts,
        workaround_flags,
        empty_retries,
        negative_savings,
    };
    let mut reports = Vec::new();
    for filter_name in filter_names {
        if let Some(only) = filter_filter
            && only != filter_name
        {
            continue;
        }
        reports.push(build_one_filter_report(
            &filter_name,
            event_counts.get(&filter_name).copied().unwrap_or(0),
            &signals,
            passthrough_lookup.get(&filter_name),
        ));
    }

    sort_reports(&mut reports, sort_by);
    reports
}

/// All the slices `build_one_filter_report` needs in one place — keeps
/// the helper under the clippy `too_many_arguments` limit.
struct SignalSlices<'a> {
    bursts: &'a [BurstRow],
    workaround_flags: &'a [WorkaroundFlagRow],
    empty_retries: &'a [EmptyRetryRow],
    negative_savings: &'a [NegativeSavingsRow],
}

/// Build the per-filter row for a single filter. Split out of
/// `build_filter_reports` to keep both functions under the 60-line clippy
/// limit.
fn build_one_filter_report(
    filter_name: &str,
    event_count: usize,
    sig: &SignalSlices<'_>,
    declared_passthrough: Option<&BTreeSet<String>>,
) -> FilterReport {
    let bursts = sig.bursts;
    let workaround_flags = sig.workaround_flags;
    let empty_retries = sig.empty_retries;
    let negative_savings = sig.negative_savings;
    let filter_bursts: Vec<&BurstRow> = bursts
        .iter()
        .filter(|b| b.filter_name == filter_name)
        .collect();
    let burst_count = filter_bursts.len();
    let max_burst_size = filter_bursts
        .iter()
        .map(|b| b.burst_size)
        .max()
        .unwrap_or(0);

    let mut untracked_workaround_flags: Vec<WorkaroundFlagSuggestion> = workaround_flags
        .iter()
        .filter(|w| w.filter_name == filter_name)
        .filter(|w| declared_passthrough.is_none_or(|set| !set.contains(&w.flag)))
        .map(|w| WorkaroundFlagSuggestion {
            flag: w.flag.clone(),
            count: w.count,
        })
        .collect();
    untracked_workaround_flags.sort_by(|a, b| b.count.cmp(&a.count).then(a.flag.cmp(&b.flag)));
    let workaround_count: usize = untracked_workaround_flags.iter().map(|w| w.count).sum();

    let empty_retry_count: usize = empty_retries
        .iter()
        .filter(|r| r.filter_name == filter_name)
        .map(|r| r.retry_count)
        .sum();

    let avg_excess_tokens = negative_savings
        .iter()
        .find(|n| n.filter_name == filter_name)
        .map(|n| n.avg_excess_tokens);

    let health_score = score_filter(
        burst_count,
        workaround_count,
        empty_retry_count,
        avg_excess_tokens,
    );

    FilterReport {
        filter_name: filter_name.to_string(),
        event_count,
        burst_count,
        max_burst_size,
        untracked_workaround_flags,
        empty_retry_count,
        avg_excess_tokens,
        health_score,
    }
}

fn sort_reports(reports: &mut [FilterReport], sort_by: SortBy) {
    match sort_by {
        SortBy::Health => {
            // Worst first (lowest score). Tie-break by filter name for stability.
            reports.sort_by(|a, b| {
                a.health_score
                    .cmp(&b.health_score)
                    .then_with(|| a.filter_name.cmp(&b.filter_name))
            });
        }
        SortBy::Bursts => {
            reports.sort_by(|a, b| {
                b.burst_count
                    .cmp(&a.burst_count)
                    .then_with(|| b.max_burst_size.cmp(&a.max_burst_size))
                    .then_with(|| a.filter_name.cmp(&b.filter_name))
            });
        }
        SortBy::Tokens => {
            reports.sort_by(|a, b| {
                let a_excess = a.avg_excess_tokens.unwrap_or(f64::NEG_INFINITY);
                let b_excess = b.avg_excess_tokens.unwrap_or(f64::NEG_INFINITY);
                b_excess
                    .partial_cmp(&a_excess)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.filter_name.cmp(&b.filter_name))
            });
        }
    }
}

/// Composite health score 0–100, **lower is worse**. Each signal contributes
/// a capped penalty so a single dimension cannot crash the score on its own.
///
/// Penalty caps (deliberately tunable from one place):
/// - bursts: up to 40
/// - workaround flags: up to 20
/// - empty retries: up to 20
/// - negative savings: up to 20
fn score_filter(
    burst_count: usize,
    workaround_count: usize,
    empty_retry_count: usize,
    avg_excess_tokens: Option<f64>,
) -> u8 {
    let burst_penalty = (burst_count.saturating_mul(2)).min(40);
    let workaround_penalty = workaround_count.min(20);
    let empty_penalty = empty_retry_count.min(20);
    let negative_penalty = avg_excess_tokens.filter(|v| *v > 0.0).map_or(0, |v| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let p = (v / 2.0).round() as usize;
        p.min(20)
    });
    let total_penalty = burst_penalty + workaround_penalty + empty_penalty + negative_penalty;
    100u8.saturating_sub(u8::try_from(total_penalty).unwrap_or(100))
}

#[cfg(test)]
mod tests;
