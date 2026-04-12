//! `tokf doctor` — post-hoc analysis of `tracking.db` to surface filters
//! that may be causing agent confusion.
//!
//! See `docs/diagnostics.md` for end-user documentation. Orchestration
//! lives here; SQL/analysis lives in `queries`, rendering in `render`,
//! noise/shape helpers in `noise`.

pub mod noise;
pub mod queries;
pub mod render;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::Context as _;
use rusqlite::Connection;

use crate::config::ResolvedFilter;

use self::queries::{
    BurstRow, EmptyChainRow, FilterStats, NegativeSavingsRow, ShapeBurstRow, WorkaroundFlagRow,
    compute_filter_stats, compute_negative_savings, detect_bursts, detect_empty_chains,
    detect_shape_bursts, detect_workaround_flags, fetch_events,
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
    #[default]
    Health,
    Bursts,
    Tokens,
}

/// One per-filter row in the doctor's report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FilterReport {
    pub filter_name: String,
    pub event_count: usize,
    // ── burst signals ──
    pub burst_count: usize,
    pub max_burst_size: usize,
    /// Ratio of events in exact-match bursts that had non-zero exit code.
    pub failed_burst_ratio: f64,
    // ── shape-burst signals ──
    pub shape_burst_count: usize,
    /// Median arg-uniqueness across shape-burst sessions (0=confusion,
    /// 1=exploration). `None` when no shape bursts were detected.
    pub median_arg_uniqueness: Option<f64>,
    // ── workaround flags ──
    pub untracked_workaround_flags: Vec<WorkaroundFlagSuggestion>,
    // ── empty chains ──
    pub empty_chain_count: usize,
    pub max_empty_chain: usize,
    // ── token signals ──
    pub avg_excess_tokens: Option<f64>,
    // ── per-filter aggregates ──
    /// Fraction of events where `--prefer-less` chose the piped output.
    pub pipe_override_rate: f64,
    /// Total filter processing time wasted in burst events (ms).
    pub burst_time_wasted_ms: i64,
    // ── composite ──
    pub health_score: u8,
}

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
    pub bursts: Vec<BurstRow>,
    pub shape_bursts: Vec<ShapeBurstRow>,
    pub empty_chains: Vec<EmptyChainRow>,
    pub negative_savings: Vec<NegativeSavingsRow>,
    pub workaround_flags: Vec<WorkaroundFlagRow>,
}

/// Run the doctor analysis end-to-end.
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
    let total = events.len();

    let mut bursts = detect_bursts(&events, opts.burst_threshold, opts.window_secs);
    let mut shape_bursts = detect_shape_bursts(&events, opts.burst_threshold, opts.window_secs);
    let mut workaround_flags = detect_workaround_flags(&events);
    let mut empty_chains = detect_empty_chains(&events, opts.window_secs);
    let mut negative_savings = compute_negative_savings(&events);
    let filter_stats = compute_filter_stats(&events);

    if let Some(only) = opts.filter_filter {
        bursts.retain(|b| b.filter_name == only);
        shape_bursts.retain(|b| b.filter_name == only);
        workaround_flags.retain(|w| w.filter_name == only);
        empty_chains.retain(|r| r.filter_name == only);
        negative_savings.retain(|n| n.filter_name == only);
    }

    let passthrough_lookup = build_passthrough_lookup(filters);

    let filter_reports = build_filter_reports(
        &filter_stats,
        &bursts,
        &shape_bursts,
        &workaround_flags,
        &empty_chains,
        &negative_savings,
        &passthrough_lookup,
        opts.filter_filter,
        opts.sort_by,
    );

    Ok(DoctorReport {
        total_events_considered: total,
        project_filter: opts.project_filter.map(ToString::to_string),
        include_noise: opts.include_noise,
        burst_threshold: opts.burst_threshold,
        window_secs: opts.window_secs,
        filters: filter_reports,
        bursts,
        shape_bursts,
        workaround_flags,
        empty_chains,
        negative_savings,
    })
}

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

/// Pre-indexed signal data — O(1) per-filter lookups instead of
/// O(filters × signals) linear scans.
struct IndexedSignals<'a> {
    bursts: HashMap<&'a str, Vec<&'a BurstRow>>,
    shape_bursts: HashMap<&'a str, Vec<&'a ShapeBurstRow>>,
    workaround_flags: HashMap<&'a str, Vec<&'a WorkaroundFlagRow>>,
    empty_chains: HashMap<&'a str, Vec<&'a EmptyChainRow>>,
    negative_savings: HashMap<&'a str, &'a NegativeSavingsRow>,
}

fn index_signals<'a>(
    bursts: &'a [BurstRow],
    shape_bursts: &'a [ShapeBurstRow],
    workaround_flags: &'a [WorkaroundFlagRow],
    empty_chains: &'a [EmptyChainRow],
    negative_savings: &'a [NegativeSavingsRow],
) -> IndexedSignals<'a> {
    let mut idx = IndexedSignals {
        bursts: HashMap::new(),
        shape_bursts: HashMap::new(),
        workaround_flags: HashMap::new(),
        empty_chains: HashMap::new(),
        negative_savings: HashMap::new(),
    };
    for b in bursts {
        idx.bursts.entry(&b.filter_name).or_default().push(b);
    }
    for b in shape_bursts {
        idx.shape_bursts.entry(&b.filter_name).or_default().push(b);
    }
    for w in workaround_flags {
        idx.workaround_flags
            .entry(&w.filter_name)
            .or_default()
            .push(w);
    }
    for c in empty_chains {
        idx.empty_chains.entry(&c.filter_name).or_default().push(c);
    }
    for n in negative_savings {
        idx.negative_savings.insert(&n.filter_name, n);
    }
    idx
}

#[allow(clippy::too_many_arguments)]
fn build_filter_reports(
    filter_stats: &BTreeMap<String, FilterStats>,
    bursts: &[BurstRow],
    shape_bursts: &[ShapeBurstRow],
    workaround_flags: &[WorkaroundFlagRow],
    empty_chains: &[EmptyChainRow],
    negative_savings: &[NegativeSavingsRow],
    passthrough_lookup: &BTreeMap<String, BTreeSet<String>>,
    filter_filter: Option<&str>,
    sort_by: SortBy,
) -> Vec<FilterReport> {
    let idx = index_signals(
        bursts,
        shape_bursts,
        workaround_flags,
        empty_chains,
        negative_savings,
    );
    let empty_bursts: Vec<&BurstRow> = Vec::new();
    let empty_shapes: Vec<&ShapeBurstRow> = Vec::new();
    let empty_flags: Vec<&WorkaroundFlagRow> = Vec::new();
    let empty_chains_v: Vec<&EmptyChainRow> = Vec::new();

    let mut reports = Vec::new();
    for (filter_name, stats) in filter_stats {
        if let Some(only) = filter_filter
            && only != filter_name.as_str()
        {
            continue;
        }
        let f = filter_name.as_str();
        reports.push(build_one_filter_report(
            filter_name,
            stats,
            idx.bursts.get(f).unwrap_or(&empty_bursts),
            idx.shape_bursts.get(f).unwrap_or(&empty_shapes),
            idx.workaround_flags.get(f).unwrap_or(&empty_flags),
            idx.empty_chains.get(f).unwrap_or(&empty_chains_v),
            idx.negative_savings.get(f).copied(),
            passthrough_lookup.get(f),
        ));
    }
    sort_reports(&mut reports, sort_by);
    reports
}

#[allow(clippy::too_many_arguments)]
fn build_one_filter_report(
    filter_name: &str,
    stats: &FilterStats,
    bursts: &[&BurstRow],
    shape_bursts: &[&ShapeBurstRow],
    workaround_flags: &[&WorkaroundFlagRow],
    empty_chains: &[&EmptyChainRow],
    neg_savings: Option<&NegativeSavingsRow>,
    declared_passthrough: Option<&BTreeSet<String>>,
) -> FilterReport {
    // ── exact-match bursts ──
    let burst_count = bursts.len();
    let total_burst_events: usize = bursts.iter().map(|b| b.burst_size).sum();
    let max_burst_size = bursts.iter().map(|b| b.burst_size).max().unwrap_or(0);
    let burst_failures: usize = bursts.iter().map(|b| b.failed_count).sum();
    #[allow(clippy::cast_precision_loss)]
    let failed_burst_ratio = if total_burst_events == 0 {
        0.0
    } else {
        burst_failures as f64 / total_burst_events as f64
    };
    let burst_time_wasted_ms: i64 = bursts.iter().map(|b| b.total_time_ms).sum();

    // ── shape bursts ──
    let shape_burst_count = shape_bursts.len();
    let median_arg_uniqueness = shape_median_uniqueness(shape_bursts);

    // ── workaround flags ──
    let (untracked_workaround_flags, workaround_count) =
        collect_workaround_flags(workaround_flags, declared_passthrough);

    // ── empty chains ──
    let empty_chain_count: usize = empty_chains.iter().map(|c| c.chain_count).sum();
    let max_empty_chain = empty_chains
        .iter()
        .map(|c| c.max_chain_length)
        .max()
        .unwrap_or(0);

    let avg_excess_tokens = neg_savings.map(|n| n.avg_excess_tokens);

    #[allow(clippy::cast_precision_loss)]
    let pipe_override_rate = if stats.event_count == 0 {
        0.0
    } else {
        stats.pipe_override_count as f64 / stats.event_count as f64
    };

    let health_score = score_filter(&ScoreInput {
        total_burst_events,
        event_count: stats.event_count,
        failed_burst_ratio,
        workaround_count,
        max_empty_chain,
        avg_excess_tokens,
        pipe_override_rate,
    });

    FilterReport {
        filter_name: filter_name.to_string(),
        event_count: stats.event_count,
        burst_count,
        max_burst_size,
        failed_burst_ratio,
        shape_burst_count,
        median_arg_uniqueness,
        untracked_workaround_flags,
        empty_chain_count,
        max_empty_chain,
        avg_excess_tokens,
        pipe_override_rate,
        burst_time_wasted_ms,
        health_score,
    }
}

fn collect_workaround_flags(
    filter_flags: &[&WorkaroundFlagRow],
    declared: Option<&BTreeSet<String>>,
) -> (Vec<WorkaroundFlagSuggestion>, usize) {
    let mut flags: Vec<WorkaroundFlagSuggestion> = filter_flags
        .iter()
        .filter(|w| declared.is_none_or(|set| !set.contains(&w.flag)))
        .map(|w| WorkaroundFlagSuggestion {
            flag: w.flag.clone(),
            count: w.count,
        })
        .collect();
    flags.sort_by(|a, b| b.count.cmp(&a.count).then(a.flag.cmp(&b.flag)));
    let total: usize = flags.iter().map(|w| w.count).sum();
    (flags, total)
}

/// Compute the median arg-uniqueness across shape-burst sessions.
/// Each session's ratio is `distinct_commands / burst_size`.
fn shape_median_uniqueness(bursts: &[&ShapeBurstRow]) -> Option<f64> {
    if bursts.is_empty() {
        return None;
    }
    let mut ratios: Vec<f64> = bursts.iter().map(|b| b.arg_uniqueness).collect();
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = ratios.len() / 2;
    if ratios.len().is_multiple_of(2) {
        Some(f64::midpoint(ratios[mid - 1], ratios[mid]))
    } else {
        Some(ratios[mid])
    }
}

fn sort_reports(reports: &mut [FilterReport], sort_by: SortBy) {
    match sort_by {
        SortBy::Health => {
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

// ─────────────────────────── health score ───────────────────────────

/// Inputs to the health-score formula, bundled to keep `score_filter`
/// under the clippy argument limit.
pub struct ScoreInput {
    pub total_burst_events: usize,
    pub event_count: usize,
    pub failed_burst_ratio: f64,
    pub workaround_count: usize,
    pub max_empty_chain: usize,
    pub avg_excess_tokens: Option<f64>,
    pub pipe_override_rate: f64,
}

/// Composite health score 0–100, **lower is worse**.
///
/// Penalty breakdown (caps are tunable from one place):
/// - **burst rate** (up to 30): `burst_events / event_count * 100`,
///   further scaled by `1 + failed_burst_ratio` so bursts where the
///   retried commands *also fail* are penalized harder.
/// - **workaround flags** (up to 15): untracked passthrough flags.
/// - **empty chains** (up to 15): longest consecutive empty-output
///   chain. A chain of 5 empties is worse than 5 isolated empties.
/// - **negative savings** (up to 15): average excess tokens.
/// - **pipe override rate** (up to 10): how often `--prefer-less`
///   chose piped output over filtered — signal that the filter is
///   producing larger output than alternatives.
/// - **time waste** (up to 15): implicit via burst rate — more burst
///   events = more filter processing time wasted. Not a separate
///   penalty to avoid double-counting, but `burst_time_wasted_ms` is
///   in the report for the user to see.
pub fn score_filter(input: &ScoreInput) -> u8 {
    #[allow(clippy::cast_precision_loss)]
    let burst_rate_pct = if input.event_count == 0 {
        0.0
    } else {
        input.total_burst_events as f64 / input.event_count as f64 * 100.0
    };
    // Scale burst penalty by failure ratio: all-failing bursts get 2×
    let failure_multiplier = 1.0 + input.failed_burst_ratio;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let burst_penalty = ((burst_rate_pct * failure_multiplier).round() as usize).min(30);

    let workaround_penalty = input.workaround_count.min(15);

    // Empty chains: penalty scales with max chain length.
    // Chain of 2 = 3 pts, chain of 5 = 10 pts, chain of 10+ = 15 pts.
    let empty_penalty = (input.max_empty_chain.saturating_mul(2)).min(15);

    let negative_penalty = input.avg_excess_tokens.filter(|v| *v > 0.0).map_or(0, |v| {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let p = (v / 3.0).round() as usize;
        p.min(15)
    });

    // Pipe override: >10% override rate → up to 10 pts penalty.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pipe_penalty = ((input.pipe_override_rate * 100.0).round() as usize).min(10);

    let total =
        burst_penalty + workaround_penalty + empty_penalty + negative_penalty + pipe_penalty;
    100u8.saturating_sub(u8::try_from(total).unwrap_or(100))
}

#[cfg(test)]
mod tests;
