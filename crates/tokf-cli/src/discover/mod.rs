pub mod parser;
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
pub mod types;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use types::{CommandAnalysis, DiscoverResult, DiscoverSummary, ExtractedCommand};

use crate::config::{self, ResolvedFilter};
use crate::rewrite::compound::{bare_pipe_positions, split_compound};
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

    // Pre-compute display names so we don't repeat PathBuf→String per command.
    let filter_names: Vec<String> = filters
        .iter()
        .map(|f| {
            f.relative_path
                .with_extension("")
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let historical_ratios = load_historical_ratios();
    let mut counters = Counters::default();
    let mut aggregated: HashMap<(String, String), AggBucket> = HashMap::new();

    let ctx = SessionContext {
        filters: &filters,
        filter_names: &filter_names,
        historical_ratios: &historical_ratios,
    };

    let mut sessions_scanned = 0usize;
    for path in session_files {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        sessions_scanned += 1;
        classify_session(file, &ctx, &mut counters, &mut aggregated);
    }

    let results = build_results(aggregated);
    let estimated_total_savings = results.iter().map(|r| r.estimated_savings).sum();

    Ok(DiscoverSummary {
        sessions_scanned,
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

struct SessionContext<'a> {
    filters: &'a [ResolvedFilter],
    filter_names: &'a [String],
    historical_ratios: &'a HashMap<String, f64>,
}

fn classify_session(
    file: std::fs::File,
    ctx: &SessionContext<'_>,
    counters: &mut Counters,
    aggregated: &mut HashMap<(String, String), AggBucket>,
) {
    let commands = parser::parse_session(file);
    counters.total += commands.len();

    for cmd in &commands {
        match classify_command(cmd, ctx.filters, ctx.filter_names) {
            CommandAnalysis::AlreadyFiltered => counters.already_filtered += 1,
            CommandAnalysis::Filterable {
                filter_name,
                normalized_command,
            } => {
                counters.filterable += 1;
                let pct = ctx
                    .historical_ratios
                    .get(&filter_name)
                    .copied()
                    .unwrap_or(DEFAULT_SAVINGS_PCT);
                let bucket = aggregated
                    .entry((filter_name, normalized_command))
                    .or_default();
                bucket.occurrences += 1;
                bucket.total_output_bytes += cmd.output_bytes;
                bucket.savings_pct = pct;
                bucket.has_filter = true;
            }
            CommandAnalysis::NoFilter => {
                counters.no_filter += 1;
                let normalized = normalize_command(&cmd.command);
                for key in extract_group_keys(&normalized) {
                    let bucket = aggregated.entry((String::new(), key)).or_default();
                    bucket.occurrences += 1;
                    bucket.total_output_bytes += cmd.output_bytes;
                }
            }
        }
    }
}

fn build_results(aggregated: HashMap<(String, String), AggBucket>) -> Vec<DiscoverResult> {
    let mut results: Vec<DiscoverResult> = aggregated
        .into_iter()
        .map(|((filter_name, command_pattern), bucket)| {
            let estimated_tokens = bucket.total_output_bytes / 4;
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let estimated_savings = (estimated_tokens as f64 * bucket.savings_pct / 100.0) as usize;
            DiscoverResult {
                command_pattern,
                filter_name,
                has_filter: bucket.has_filter,
                occurrences: bucket.occurrences,
                total_output_bytes: bucket.total_output_bytes,
                estimated_tokens,
                estimated_savings,
                savings_pct: bucket.savings_pct,
            }
        })
        .collect();

    // Sort by tokens (most waste first). For filtered commands, use savings;
    // for unfiltered, use total tokens since we can't estimate savings.
    results.sort_by(|a, b| {
        let a_key = if a.has_filter {
            a.estimated_savings
        } else {
            a.estimated_tokens
        };
        let b_key = if b.has_filter {
            b.estimated_savings
        } else {
            b.estimated_tokens
        };
        b_key.cmp(&a_key)
    });
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
/// `/Users/foo/github.com/project` → `-Users-foo-github-com-project`
///
/// Claude Code replaces both `/` and `.` with `-` in the encoded path.
pub fn encode_project_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_start_matches('/').trim_end_matches('/');
    let encoded: String = trimmed
        .chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect();
    format!("-{encoded}")
}

/// Classify a single command against the available filters.
///
/// `filter_names` must be parallel to `filters` (pre-computed display names).
pub fn classify_command(
    cmd: &ExtractedCommand,
    filters: &[ResolvedFilter],
    filter_names: &[String],
) -> CommandAnalysis {
    let command = cmd.command.trim();

    if command.starts_with("tokf run ") {
        return CommandAnalysis::AlreadyFiltered;
    }

    let normalized = normalize_command(command);
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if words.is_empty() {
        return CommandAnalysis::NoFilter;
    }

    for (filter, name) in filters.iter().zip(filter_names) {
        if filter.matches(&words).is_some() {
            return CommandAnalysis::Filterable {
                filter_name: name.clone(),
                normalized_command: normalized,
            };
        }
    }

    CommandAnalysis::NoFilter
}

/// Normalize a command for matching: strip the last bare pipe and everything after it.
fn normalize_command(command: &str) -> String {
    let command = command.trim();
    // Reuse the existing quote-aware pipe finder from rewrite::compound.
    let positions = bare_pipe_positions(command);
    if let Some(&last_pipe) = positions.last() {
        command[..last_pipe].trim_end().to_string()
    } else {
        command.to_string()
    }
}

/// Split a compound command into segments and extract a group key for each.
///
/// Compound commands like `cd /tmp && gh repo view ...` yield multiple keys:
/// `["cd", "gh repo view"]`. Each segment is independently grouped.
///
/// Uses `split_compound` for `&&`/`||`/`;` splitting. Takes only the first
/// line to avoid parsing heredoc content or multi-line strings.
fn extract_group_keys(command: &str) -> Vec<String> {
    // Take only the first line — heredocs and multi-line commands have
    // their actual command on line 1.
    let first_line = command.lines().next().unwrap_or(command);
    // Strip heredoc markers and everything after them.
    let first_line = strip_heredoc(first_line);
    let segments = split_compound(&first_line);
    segments
        .iter()
        .filter_map(|(segment, _)| {
            let trimmed = segment.trim();
            if trimmed.is_empty() {
                return None;
            }
            let key = command_group_key(trimmed);
            if key.is_empty() { None } else { Some(key) }
        })
        .collect()
}

/// Extract a smart grouping key from a single command.
///
/// Takes the basename of the program and appends subcommand-like words
/// (non-flag, non-path tokens) up to a reasonable depth:
/// - `gh pr list --limit 5` → `gh pr list`
/// - `cargo test --workspace` → `cargo test`
/// - `find /Users/... -name "*.rs"` → `find`
/// - `git log --oneline -5` → `git log`
/// - `RUST_LOG=debug cargo test` → `cargo test` (strips env vars)
/// - `python manage.py migrate` → `python manage.py migrate`
fn command_group_key(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.is_empty() {
        return String::new();
    }

    // Skip leading env var assignments (KEY=value).
    let start = words.iter().position(|w| !w.contains('=')).unwrap_or(0);
    let words = &words[start..];
    if words.is_empty() {
        return String::new();
    }

    // Take basename of the program.
    let program = words[0].rsplit('/').next().unwrap_or(words[0]);

    // Skip non-command-like first words (code fragments, punctuation, etc.)
    if program.is_empty()
        || program.starts_with('"')
        || program.starts_with('\'')
        || program.starts_with('(')
        || program.starts_with(')')
        || program.starts_with('{')
        || program.starts_with('#')
        || program.starts_with('*')
        || program.contains('(')
        || program.contains('|')
        || program.contains('\\')
        || program.contains('>')
        || program.contains('<')
        || !program.chars().any(char::is_alphanumeric)
    {
        return String::new();
    }

    let mut key_parts = vec![program];

    // Collect subcommand-like words: non-flag, non-path, non-quoted.
    for word in words.iter().skip(1) {
        if word.starts_with('-')
            || word.starts_with('/')
            || word.starts_with('~')
            || word.starts_with('.')
            || word.starts_with('"')
            || word.starts_with('\'')
            || word.starts_with('$')
            || word.contains('/')
            || word.contains('=')
            || word.contains('*')
            || word.contains('{')
        {
            break;
        }
        key_parts.push(word);
        // Most tools have at most 2 levels of subcommands (e.g. `gh pr list`).
        if key_parts.len() >= 3 {
            break;
        }
    }

    key_parts.join(" ")
}

/// Strip heredoc markers (`<<EOF`, `<<'EOF'`, `<<"EOF"`) and everything after.
fn strip_heredoc(line: &str) -> String {
    line.find("<<").map_or_else(
        || line.to_string(),
        |idx| line[..idx].trim_end().to_string(),
    )
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
    has_filter: bool,
}
