//! `tokf doctor` CLI entry point. Mirrors the convention used by
//! `discover_cmd.rs` — a flat `*Opts` struct + a `cmd_*` dispatch fn.

use std::io::IsTerminal as _;

use tokf::doctor::render::{Colors, render_human, should_disable_color};
use tokf::doctor::{DoctorOpts, SortBy, run};
use tokf::tracking;

use crate::resolve;

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are naturally booleans
pub struct DoctorCliOpts<'a> {
    pub burst_threshold: usize,
    pub window_secs: u64,
    pub project: Option<&'a str>,
    pub all_projects: bool,
    pub include_noise: bool,
    pub filter: Option<&'a str>,
    pub sort: SortBy,
    pub json: bool,
    pub no_color: bool,
    pub no_cache: bool,
}

/// Run the doctor command. Returns the process exit code.
///
/// Exit codes:
///   - `0`: report rendered successfully (even if it surfaced problems)
///   - `1`: failed to open the tracking DB or fetch events
pub fn cmd_doctor(opts: &DoctorCliOpts<'_>) -> i32 {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine tracking DB path");
        return 1;
    };
    let conn = match tracking::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening tracking DB: {e:#}");
            return 1;
        }
    };

    // Determine the project filter:
    //   - --all → None (no filter)
    //   - --project <p> → Some(p)
    //   - default → Some(current project) so the report is scoped to the
    //     repo the user is currently inside
    let resolved_project: Option<String> = if opts.all_projects {
        None
    } else if let Some(p) = opts.project {
        Some(p.to_string())
    } else {
        Some(tokf::history::current_project())
    };

    let doctor_opts = DoctorOpts {
        burst_threshold: opts.burst_threshold,
        window_secs: opts.window_secs,
        project_filter: resolved_project.as_deref(),
        include_noise: opts.include_noise,
        filter_filter: opts.filter,
        sort_by: opts.sort,
    };

    // Cross-reference workaround flags against each filter's
    // passthrough_args. Failures here are non-fatal — we just lose the
    // suggestion enrichment, the rest of the report still works.
    let filters = resolve::discover_filters(opts.no_cache).unwrap_or_default();

    // `--filter` accepts either the slash-form filter name (`git/diff`) or
    // the command pattern (`git diff`). The DB stores the latter — if the
    // user passed the former, look up the matching filter and substitute.
    let normalized_filter: Option<String> = opts.filter.map(|requested| {
        filters
            .iter()
            .find(|f| f.relative_path.with_extension("").to_string_lossy() == requested)
            .map_or_else(
                || requested.to_string(),
                |f| f.config.command.first().to_string(),
            )
    });
    let doctor_opts = DoctorOpts {
        filter_filter: normalized_filter.as_deref(),
        ..doctor_opts
    };

    let report = match run(&conn, &doctor_opts, &filters) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tokf] doctor: {e:#}");
            return 1;
        }
    };

    if opts.json {
        crate::output::print_json(&report);
        return 0;
    }

    let colors = if should_disable_color(opts.no_color) || !std::io::stdout().is_terminal() {
        Colors::disabled()
    } else {
        Colors::enabled()
    };
    print!("{}", render_human(&report, &colors));
    0
}
