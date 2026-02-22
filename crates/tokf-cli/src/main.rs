mod cache_cmd;
mod eject_cmd;
mod gain;
mod history_cmd;
mod resolve;
mod verify_cmd;

use std::path::Path;

use clap::{Parser, Subcommand};

use tokf::baseline;
use tokf::config;
use tokf::filter;
use tokf::history;
use tokf::hook;
use tokf::rewrite;
use tokf::runner;
use tokf::skill;

#[derive(Parser)]
#[command(
    name = "tokf",
    version,
    about = "Token filter — compress command output for LLM context"
)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are naturally booleans
struct Cli {
    /// Show how long filtering took
    #[arg(long, global = true)]
    timing: bool,

    /// Skip filtering, pass output through raw
    #[arg(long, global = true)]
    no_filter: bool,

    /// Show filter resolution details
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Bypass the binary config cache for this invocation
    #[arg(long, global = true)]
    no_cache: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a command and filter its output
    Run {
        /// Pipe command for fair accounting (set by rewrite when stripping pipes)
        #[arg(long)]
        baseline_pipe: Option<String>,
        #[arg(trailing_var_arg = true, required = true)]
        command_args: Vec<String>,
    },
    /// Validate a filter TOML file
    Check {
        /// Path to the filter file
        filter_path: String,
    },
    /// Apply a filter to a fixture file
    Test {
        /// Path to the filter file
        filter_path: String,
        /// Path to the fixture file
        fixture_path: String,
        /// Simulated exit code for branch selection
        #[arg(long, default_value_t = 0)]
        exit_code: i32,
    },
    /// List available filters
    Ls,
    /// Rewrite a command string (apply filter-derived rules)
    Rewrite {
        /// The command string to rewrite
        command: String,
    },
    /// Show which filter would be used for a command
    Which {
        /// The command string to look up (e.g. "git push origin main")
        command: String,
    },
    /// Show the TOML source of an active filter
    Show {
        /// Filter relative path without extension (e.g. "git/push")
        filter: String,
    },
    /// Copy a filter to your local or global config for customization
    Eject {
        /// Filter relative path without extension (e.g. "cargo/build")
        filter: String,
        /// Eject to global config dir instead of project-local .tokf/
        #[arg(long)]
        global: bool,
    },
    /// Claude Code hook management
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Install the Claude Code filter-authoring skill
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Manage the filter resolution cache
    Cache {
        #[command(subcommand)]
        action: cache_cmd::CacheAction,
    },
    /// Show token savings statistics
    Gain {
        /// Show daily breakdown
        #[arg(long)]
        daily: bool,
        /// Show breakdown by filter
        #[arg(long, name = "by-filter")]
        by_filter: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage filtered output history
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Run declarative test suites for filters
    Verify {
        /// Filter name to test (e.g. "cargo/build"). Omit to run all.
        filter: Option<String>,
        /// List available test suites without running them
        #[arg(long)]
        list: bool,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Fail if any filters have no test suite
        #[arg(long)]
        require_all: bool,
    },
}

#[derive(Subcommand)]
enum SkillAction {
    /// Install skill files to .claude/skills/tokf-filter/ (project-local or global)
    Install {
        /// Install globally (~/.claude/skills/) instead of project-local (.claude/skills/)
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle a `PreToolUse` hook invocation (reads JSON from stdin)
    Handle,
    /// Install the hook into Claude Code settings
    Install {
        /// Install globally (~/.config/tokf) instead of project-local (.tokf)
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
enum HistoryAction {
    /// List recent history entries (current project by default)
    List {
        /// Number of entries to show (default: 10)
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Show history from all projects
        #[arg(short, long)]
        all: bool,
    },
    /// Show details of a specific history entry
    Show {
        /// Entry ID to show
        id: i64,
    },
    /// Search history by command or output content (current project by default)
    Search {
        /// Search query (searches command, raw output, and filtered output)
        query: String,
        /// Maximum number of results to show (default: 10)
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Search across all projects
        #[arg(short, long)]
        all: bool,
    },
    /// Clear history entries (current project by default)
    Clear {
        /// Clear history for all projects — this is destructive and cannot be undone
        #[arg(short, long)]
        all: bool,
    },
}

// NOTE: cmd_run integrates command resolution, execution, output rendering, tracking,
// and history recording. Splitting would require threading 5+ values through helpers.
// Approved to exceed the 60-line limit.
#[allow(clippy::too_many_lines)]
fn cmd_run(command_args: &[String], baseline_pipe: Option<&str>, cli: &Cli) -> anyhow::Result<i32> {
    let filter_match = if cli.no_filter {
        None
    } else {
        resolve::find_filter(command_args, cli.verbose, cli.no_cache)?
    };

    let words_consumed = filter_match.as_ref().map_or(0, |m| m.words_consumed);
    let remaining_args: Vec<String> = if words_consumed > 0 {
        command_args[words_consumed..].to_vec()
    } else if command_args.len() > 1 {
        command_args[1..].to_vec()
    } else {
        vec![]
    };

    let filter_cfg = filter_match.as_ref().map(|m| &m.config);
    let cmd_result =
        resolve::run_command(filter_cfg, words_consumed, command_args, &remaining_args)?;

    let Some(filter_match) = filter_match else {
        let raw_len = cmd_result.combined.len();
        let input_bytes = match baseline_pipe {
            Some(pipe_cmd) => baseline::compute(&cmd_result.combined, pipe_cmd),
            None => raw_len,
        };
        if !cmd_result.combined.is_empty() {
            println!("{}", cmd_result.combined);
        }
        // filter_time_ms = 0: no filter was applied, not 0ms of filtering.
        // Passthrough commands are not recorded to history: raw == filtered would
        // waste storage and add noise with nothing useful to compare.
        // output_bytes = raw_len: what tokf actually printed (full raw output).
        resolve::record_run(
            command_args,
            None,
            input_bytes,
            raw_len,
            0,
            cmd_result.exit_code,
        );
        return Ok(cmd_result.exit_code);
    };

    // Phase B: resolve deferred output-pattern variants using the already-discovered
    // filter list (no second discovery call needed).
    let cfg = resolve::resolve_phase_b(filter_match, &cmd_result.combined, cli.verbose);

    let input_bytes = match baseline_pipe {
        Some(pipe_cmd) => baseline::compute(&cmd_result.combined, pipe_cmd),
        None => cmd_result.combined.len(),
    };
    let start = std::time::Instant::now();
    let filtered = filter::apply(&cfg, &cmd_result, &remaining_args);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    let output_bytes = filtered.output.len();
    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

    let filter_name = cfg.command.first();
    resolve::record_run(
        command_args,
        Some(filter_name),
        input_bytes,
        output_bytes,
        elapsed.as_millis(),
        cmd_result.exit_code,
    );

    history::try_record(
        &command_args.join(" "),
        filter_name,
        &cmd_result.combined,
        &filtered.output,
        cmd_result.exit_code,
    );

    Ok(cmd_result.exit_code)
}

fn cmd_check(filter_path: &Path) -> i32 {
    match config::try_load_filter(filter_path) {
        Ok(Some(cfg)) => {
            eprintln!(
                "[tokf] {} is valid (command: \"{}\")",
                filter_path.display(),
                cfg.command.first()
            );
            0
        }
        Ok(None) => {
            eprintln!("[tokf] file not found: {}", filter_path.display());
            1
        }
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn cmd_test(
    filter_path: &Path,
    fixture_path: &Path,
    exit_code: i32,
    cli: &Cli,
) -> anyhow::Result<i32> {
    let cfg = config::try_load_filter(filter_path)?
        .ok_or_else(|| anyhow::anyhow!("filter not found: {}", filter_path.display()))?;

    let fixture = std::fs::read_to_string(fixture_path)
        .map_err(|e| anyhow::anyhow!("failed to read fixture: {}: {e}", fixture_path.display()))?;
    let combined = fixture.trim_end().to_string();

    let cmd_result = runner::CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code,
        combined,
    };

    let start = std::time::Instant::now();
    let filtered = filter::apply(&cfg, &cmd_result, &[]);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

    Ok(0)
}

// Note: cmd_ls and cmd_which always use the cache. The --no-cache flag
// only affects `tokf run`. Pass --no-cache to `tokf run` if you need uncached resolution.
fn cmd_ls(verbose: bool) -> i32 {
    let Ok(filters) = resolve::discover_filters(false) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    for filter in &filters {
        // Display: relative path without .toml extension  →  command
        let display_name = filter
            .relative_path
            .with_extension("")
            .display()
            .to_string();
        println!(
            "{display_name}  \u{2192}  {}",
            filter.config.command.first()
        );

        if verbose {
            eprintln!(
                "[tokf]   source: {}  [{}]",
                filter.source_path.display(),
                filter.priority_label()
            );
            let patterns = filter.config.command.patterns();
            if patterns.len() > 1 {
                for p in patterns {
                    eprintln!("[tokf]     pattern: \"{p}\"");
                }
            }
        }
    }

    0
}

fn cmd_which(command: &str, verbose: bool) -> i32 {
    let Ok(filters) = resolve::discover_filters(false) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    let words: Vec<&str> = command.split_whitespace().collect();
    let cwd = std::env::current_dir().unwrap_or_default();

    for filter in &filters {
        if filter.matches(&words).is_some() {
            let display_name = filter
                .relative_path
                .with_extension("")
                .display()
                .to_string();

            let variant_info = if filter.config.variant.is_empty() {
                String::new()
            } else {
                let res =
                    config::variant::resolve_variants(&filter.config, &filters, &cwd, verbose);
                let resolved = res.config.command.first().to_string();
                if resolved != filter.config.command.first() {
                    format!(" -> variant: \"{resolved}\"")
                } else if res.output_variants.is_empty() {
                    format!(
                        " ({} variant(s), none matched by file)",
                        filter.config.variant.len()
                    )
                } else {
                    let names: Vec<&str> = res
                        .output_variants
                        .iter()
                        .map(|v| v.name.as_str())
                        .collect();
                    format!(
                        " ({} variant(s), {} deferred to output-pattern: {})",
                        filter.config.variant.len(),
                        res.output_variants.len(),
                        names.join(", ")
                    )
                }
            };
            println!(
                "{display_name}  [{}]  command: \"{}\"{variant_info}",
                filter.priority_label(),
                filter.config.command.first()
            );
            if verbose {
                eprintln!("[tokf] source: {}", filter.source_path.display());
            }
            return 0;
        }
    }

    eprintln!("[tokf] no filter found for \"{command}\"");
    1
}

fn or_exit(r: anyhow::Result<i32>) -> i32 {
    r.unwrap_or_else(|e| {
        eprintln!("[tokf] error: {e:#}");
        1
    })
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match &cli.command {
        Commands::Run {
            command_args,
            baseline_pipe,
        } => or_exit(cmd_run(command_args, baseline_pipe.as_deref(), &cli)),
        Commands::Check { filter_path } => cmd_check(Path::new(filter_path)),
        Commands::Test {
            filter_path,
            fixture_path,
            exit_code,
        } => or_exit(cmd_test(
            Path::new(filter_path),
            Path::new(fixture_path),
            *exit_code,
            &cli,
        )),
        Commands::Ls => cmd_ls(cli.verbose),
        Commands::Rewrite { command } => cmd_rewrite(command, cli.verbose),
        Commands::Which { command } => cmd_which(command, cli.verbose),
        Commands::Show { filter } => cmd_show(filter),
        Commands::Eject { filter, global } => eject_cmd::cmd_eject(filter, *global, cli.no_cache),
        Commands::Hook { action } => match action {
            HookAction::Handle => cmd_hook_handle(),
            HookAction::Install { global } => cmd_hook_install(*global),
        },
        Commands::Skill { action } => match action {
            SkillAction::Install { global } => cmd_skill_install(*global),
        },
        Commands::Cache { action } => cache_cmd::run_cache_action(action),
        Commands::Gain {
            daily,
            by_filter,
            json,
        } => gain::cmd_gain(*daily, *by_filter, *json),
        Commands::Verify {
            filter,
            list,
            json,
            require_all,
        } => verify_cmd::cmd_verify(filter.as_deref(), *list, *json, *require_all),
        Commands::History { action } => or_exit(match action {
            HistoryAction::List { limit, all } => history_cmd::cmd_history_list(*limit, *all),
            HistoryAction::Show { id } => history_cmd::cmd_history_show(*id),
            HistoryAction::Search { query, limit, all } => {
                history_cmd::cmd_history_search(query, *limit, *all)
            }
            HistoryAction::Clear { all } => history_cmd::cmd_history_clear(*all),
        }),
    };
    std::process::exit(exit_code);
}

fn cmd_show(filter: &str) -> i32 {
    // Normalize: strip ".toml" suffix if present
    let filter_name = filter.strip_suffix(".toml").unwrap_or(filter);

    let Ok(filters) = resolve::discover_filters(false) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    let found = filters
        .iter()
        .find(|f| f.relative_path.with_extension("").to_string_lossy() == filter_name);

    let Some(resolved) = found else {
        eprintln!("[tokf] filter not found: {filter}");
        return 1;
    };

    let content = if resolved.priority == u8::MAX {
        if let Some(c) = config::get_embedded_filter(&resolved.relative_path) {
            c.to_string()
        } else {
            eprintln!("[tokf] error: embedded filter not readable");
            return 1;
        }
    } else {
        match std::fs::read_to_string(&resolved.source_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[tokf] error reading filter: {e}");
                return 1;
            }
        }
    };

    print!("{content}");
    0
}

fn cmd_rewrite(command: &str, verbose: bool) -> i32 {
    let result = rewrite::rewrite(command, verbose);
    println!("{result}");
    0
}

fn cmd_skill_install(global: bool) -> i32 {
    match skill::install(global) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn cmd_hook_handle() -> i32 {
    hook::handle();
    0
}

fn cmd_hook_install(global: bool) -> i32 {
    match hook::install(global) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}
