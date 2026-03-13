pub mod parser;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
pub mod types;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use types::{CommandAnalysis, DiscoverResult, DiscoverSummary, ExtractedCommand};

use crate::config::{self, ResolvedFilter};
use crate::tracking;

const DEFAULT_SAVINGS_PCT: f64 = 60.0;

/// Discover missed token savings across Claude Code session files.
///
/// # Errors
///
/// Returns an error if filter discovery fails.
pub fn discover_sessions(
    session_files: &[PathBuf],
    no_cache: bool,
) -> anyhow::Result<DiscoverSummary> {
    let search_dirs = config::default_search_dirs();
    let filters = if no_cache {
        config::discover_all_filters(&search_dirs)?
    } else {
        config::cache::discover_with_cache(&search_dirs)?
    };

    let historical_ratios = load_historical_ratios();
    let mut counters = Counters::default();
    let mut aggregated: HashMap<(String, String), AggBucket> = HashMap::new();

    for path in session_files {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        classify_session(
            file,
            &filters,
            &historical_ratios,
            &mut counters,
            &mut aggregated,
        );
    }

    let results = build_results(aggregated);
    let estimated_total_savings = results.iter().map(|r| r.estimated_savings).sum();

    Ok(DiscoverSummary {
        sessions_scanned: session_files.len(),
        total_commands: counters.total,
        already_filtered: counters.already_filtered,
        filterable_commands: counters.filterable,
        no_filter_commands: counters.no_filter,
        estimated_total_savings,
        results,
    })
}

#[derive(Default)]
struct Counters {
    total: usize,
    already_filtered: usize,
    filterable: usize,
    no_filter: usize,
}

fn classify_session(
    file: std::fs::File,
    filters: &[ResolvedFilter],
    historical_ratios: &HashMap<String, f64>,
    counters: &mut Counters,
    aggregated: &mut HashMap<(String, String), AggBucket>,
) {
    let commands = parser::parse_session(file);
    counters.total += commands.len();

    for cmd in &commands {
        match classify_command(cmd, filters) {
            CommandAnalysis::AlreadyFiltered => counters.already_filtered += 1,
            CommandAnalysis::Filterable {
                filter_name,
                savings_pct: _,
            } => {
                counters.filterable += 1;
                let norm = normalize_command(&cmd.command);
                let pct = historical_ratios
                    .get(&filter_name)
                    .copied()
                    .unwrap_or(DEFAULT_SAVINGS_PCT);
                let bucket = aggregated.entry((filter_name, norm)).or_default();
                bucket.occurrences += 1;
                bucket.total_output_bytes += cmd.output_bytes;
                bucket.savings_pct = pct;
            }
            CommandAnalysis::NoFilter => counters.no_filter += 1,
        }
    }
}

fn build_results(aggregated: HashMap<(String, String), AggBucket>) -> Vec<DiscoverResult> {
    let mut results: Vec<DiscoverResult> = aggregated
        .into_iter()
        .map(|((filter_name, command_pattern), bucket)| {
            let estimated_tokens = bucket.total_output_bytes / 4;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let estimated_savings = (f64::from(u32::try_from(estimated_tokens).unwrap_or(u32::MAX))
                * bucket.savings_pct
                / 100.0) as usize;
            DiscoverResult {
                command_pattern,
                filter_name,
                occurrences: bucket.occurrences,
                total_output_bytes: bucket.total_output_bytes,
                estimated_tokens,
                estimated_savings,
                savings_pct: bucket.savings_pct,
            }
        })
        .collect();

    results.sort_by(|a, b| b.estimated_savings.cmp(&a.estimated_savings));
    results
}

/// Enumerate Claude Code session JSONL files for a specific project path.
pub fn session_files_for_project(project_path: &Path) -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let encoded = encode_project_path(project_path);
    let session_dir = home.join(".claude/projects").join(encoded);
    list_jsonl_files(&session_dir)
}

/// Enumerate all Claude Code session JSONL files across all projects.
pub fn all_session_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };
    let projects_dir = home.join(".claude/projects");
    let Ok(entries) = std::fs::read_dir(&projects_dir) else {
        return vec![];
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            files.extend(list_jsonl_files(&entry.path()));
        }
    }
    files
}

fn list_jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .map(|e| e.path())
        .collect()
}

/// Encode an absolute project path for Claude Code's directory naming.
/// `/Users/foo/project` → `-Users-foo-project`
pub fn encode_project_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_start_matches('/').trim_end_matches('/');
    format!("-{}", trimmed.replace('/', "-"))
}

/// Classify a single command against the available filters.
pub fn classify_command(cmd: &ExtractedCommand, filters: &[ResolvedFilter]) -> CommandAnalysis {
    let command = cmd.command.trim();

    if command.starts_with("tokf run ") {
        return CommandAnalysis::AlreadyFiltered;
    }

    let normalized = normalize_command(command);
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.is_empty() {
        return CommandAnalysis::NoFilter;
    }

    for filter in filters {
        if filter.matches(&words).is_some() {
            let name = filter
                .relative_path
                .with_extension("")
                .to_string_lossy()
                .to_string();
            return CommandAnalysis::Filterable {
                filter_name: name,
                savings_pct: DEFAULT_SAVINGS_PCT,
            };
        }
    }

    CommandAnalysis::NoFilter
}

/// Normalize a command for matching: strip trailing pipes, redirections, etc.
fn normalize_command(command: &str) -> String {
    let command = command.trim();
    // Strip trailing pipe chains (e.g. `| tail -20`, `| head -n 5`, `2>&1 | grep`)
    let mut result = command.to_string();
    if let Some(idx) = find_trailing_pipe(&result) {
        result.truncate(idx);
        result = result.trim_end().to_string();
    }
    result
}

/// Find the index of the first `|` that starts a trailing pipe chain.
fn find_trailing_pipe(command: &str) -> Option<usize> {
    // Simple heuristic: find last `|` not inside quotes
    let mut in_single = false;
    let mut in_double = false;
    let mut last_pipe = None;

    for (i, ch) in command.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '|' if !in_single && !in_double => {
                // Skip `||` (logical OR)
                if command.as_bytes().get(i + 1) == Some(&b'|') {
                    continue;
                }
                // Skip if preceded by `|` (second char of `||` already handled)
                if i > 0 && command.as_bytes().get(i - 1) == Some(&b'|') {
                    continue;
                }
                last_pipe = Some(i);
            }
            _ => {}
        }
    }
    last_pipe
}

/// Load historical savings ratios from the tracking database (`filter_name` → `savings_pct`).
fn load_historical_ratios() -> HashMap<String, f64> {
    let mut ratios = HashMap::new();
    let Some(db_path) = tracking::db_path() else {
        return ratios;
    };
    let Ok(conn) = tracking::open_db(&db_path) else {
        return ratios;
    };
    let Ok(gains) = tracking::query_by_filter(&conn) else {
        return ratios;
    };
    for gain in gains {
        if gain.savings_pct > 0.0 {
            ratios.insert(gain.filter_name, gain.savings_pct);
        }
    }
    ratios
}

#[derive(Default)]
struct AggBucket {
    occurrences: usize,
    total_output_bytes: usize,
    savings_pct: f64,
}
