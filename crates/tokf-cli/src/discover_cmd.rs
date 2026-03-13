use std::path::PathBuf;

use tokf::discover;

pub struct DiscoverOpts<'a> {
    pub project: Option<&'a str>,
    pub all: bool,
    pub session: Option<&'a str>,
    pub since: Option<&'a str>,
    pub limit: usize,
    pub json: bool,
    pub no_cache: bool,
}

pub fn cmd_discover(opts: &DiscoverOpts<'_>) -> anyhow::Result<i32> {
    let session_files = collect_session_files(opts.project, opts.all, opts.session)?;

    if session_files.is_empty() {
        eprintln!("[tokf] no session files found");
        return Ok(1);
    }

    let filtered_files = apply_since_filter(&session_files, opts.since);

    if filtered_files.is_empty() {
        eprintln!("[tokf] no session files match --since filter");
        return Ok(1);
    }

    let summary = discover::discover_sessions(&filtered_files, opts.no_cache)?;

    if opts.json {
        crate::output::print_json(&summary);
    } else {
        print_human(&summary, opts.limit);
    }

    Ok(0)
}

fn collect_session_files(
    project: Option<&str>,
    all: bool,
    session: Option<&str>,
) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(path) = session {
        let p = PathBuf::from(path);
        if !p.exists() {
            anyhow::bail!("session file not found: {path}");
        }
        return Ok(vec![p]);
    }

    if all {
        return Ok(discover::all_session_files());
    }

    let project_path = if let Some(p) = project {
        std::fs::canonicalize(p)
            .map_err(|e| anyhow::anyhow!("cannot resolve project path '{p}': {e}"))?
    } else {
        std::env::current_dir()?
    };

    Ok(discover::session_files_for_project(&project_path))
}

fn apply_since_filter(files: &[PathBuf], since: Option<&str>) -> Vec<PathBuf> {
    let Some(since_str) = since else {
        return files.to_vec();
    };

    let Some(cutoff) = parse_since(since_str) else {
        eprintln!("[tokf] invalid --since format: {since_str} (expected e.g. 7d, 24h)");
        return files.to_vec();
    };

    files
        .iter()
        .filter(|f| {
            f.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|mtime| mtime >= cutoff)
        })
        .cloned()
        .collect()
}

fn parse_since(s: &str) -> Option<std::time::SystemTime> {
    let s = s.trim();
    let (num_str, unit) = s.split_at(s.len().checked_sub(1)?);
    let num: u64 = num_str.parse().ok()?;
    let secs = match unit {
        "d" => num * 86400,
        "h" => num * 3600,
        "m" => num * 60,
        _ => return None,
    };
    std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(secs))
}

fn print_human(summary: &discover::types::DiscoverSummary, limit: usize) {
    eprintln!(
        "[tokf] scanned {} session{}, {} command{} total",
        summary.sessions_scanned,
        plural(summary.sessions_scanned),
        summary.total_commands,
        plural(summary.total_commands),
    );

    if summary.already_filtered > 0 {
        eprintln!(
            "[tokf] {} already filtered by tokf",
            summary.already_filtered,
        );
    }

    if summary.results.is_empty() {
        println!("No filterable commands found — you're all set!");
        return;
    }

    print_results_table(&summary.results, limit);

    println!();
    println!(
        "Estimated total savings: {} tokens ({} filterable, {} with no filter)",
        format_tokens(summary.estimated_total_savings),
        summary.filterable_commands,
        summary.no_filter_commands,
    );
}

const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn print_results_table(results: &[discover::types::DiscoverResult], limit: usize) {
    println!();
    println!(
        "{:<30} {:<20} {:>5} {:>10} {:>10}",
        "COMMAND", "FILTER", "RUNS", "TOKENS", "SAVINGS"
    );
    println!("{}", "-".repeat(80));

    let display_count = if limit == 0 {
        results.len()
    } else {
        limit.min(results.len())
    };

    for result in results.iter().take(display_count) {
        let cmd_display = if result.command_pattern.len() > 28 {
            let truncated: String = result.command_pattern.chars().take(25).collect();
            format!("{truncated}...")
        } else {
            result.command_pattern.clone()
        };
        println!(
            "{:<30} {:<20} {:>5} {:>10} {:>10}",
            cmd_display,
            result.filter_name,
            result.occurrences,
            format_tokens(result.estimated_tokens),
            format_tokens(result.estimated_savings),
        );
    }

    if results.len() > display_count {
        println!(
            "  ... and {} more (use --limit 0 to show all)",
            results.len() - display_count
        );
    }
}

fn format_tokens(n: usize) -> String {
    if n >= 1_000_000 {
        #[allow(clippy::cast_precision_loss)]
        let m = n as f64 / 1_000_000.0;
        format!("{m:.1}M")
    } else if n >= 1_000 {
        #[allow(clippy::cast_precision_loss)]
        let k = n as f64 / 1_000.0;
        format!("{k:.1}k")
    } else {
        n.to_string()
    }
}
