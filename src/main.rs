use std::path::Path;

use clap::{Parser, Subcommand};

use tokf::config;
use tokf::config::types::FilterConfig;
use tokf::filter;
use tokf::hook;
use tokf::rewrite;
use tokf::runner;

#[derive(Parser)]
#[command(
    name = "tokf",
    about = "Token filter — compress command output for LLM context"
)]
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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a command and filter its output
    Run {
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
    /// Claude Code hook management
    Hook {
        #[command(subcommand)]
        action: HookAction,
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

/// Find the first filter that matches `command_args` using the discovery model.
/// Returns `(Option<FilterConfig>, words_consumed)`.
fn find_filter(
    command_args: &[String],
    verbose: bool,
) -> anyhow::Result<(Option<FilterConfig>, usize)> {
    let search_dirs = config::default_search_dirs();
    let resolved = config::discover_all_filters(&search_dirs)?;
    let words: Vec<&str> = command_args.iter().map(String::as_str).collect();

    for filter in &resolved {
        if let Some(consumed) = filter.matches(&words) {
            if verbose {
                eprintln!(
                    "[tokf] matched {} (command: \"{}\") in {}",
                    filter.relative_path.display(),
                    filter.config.command.first(),
                    filter
                        .source_path
                        .parent()
                        .map_or("?", |p| p.to_str().unwrap_or("?")),
                );
            }
            return Ok((Some(filter.config.clone()), consumed));
        }
    }

    if verbose {
        eprintln!(
            "[tokf] no filter found for '{}', passing through",
            words.join(" ")
        );
    }
    Ok((None, 0))
}

fn cmd_run(command_args: &[String], cli: &Cli) -> anyhow::Result<i32> {
    let (filter_cfg, words_consumed) = if cli.no_filter {
        (None, 0)
    } else {
        find_filter(command_args, cli.verbose)?
    };

    let remaining_args: Vec<String> = if words_consumed > 0 {
        command_args[words_consumed..].to_vec()
    } else if command_args.len() > 1 {
        command_args[1..].to_vec()
    } else {
        vec![]
    };

    let cmd_result = if let Some(ref cfg) = filter_cfg
        && let Some(ref run_cmd) = cfg.run
    {
        runner::execute_shell(run_cmd, &remaining_args)?
    } else if words_consumed > 0 {
        let cmd_str = command_args[..words_consumed].join(" ");
        runner::execute(&cmd_str, &remaining_args)?
    } else {
        runner::execute(&command_args[0], &remaining_args)?
    };

    let Some(cfg) = filter_cfg else {
        if !cmd_result.combined.is_empty() {
            println!("{}", cmd_result.combined);
        }
        return Ok(cmd_result.exit_code);
    };

    let start = std::time::Instant::now();
    let filtered = filter::apply(&cfg, &cmd_result);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

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
    let filtered = filter::apply(&cfg, &cmd_result);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

    Ok(0)
}

fn cmd_ls(verbose: bool) -> i32 {
    let search_dirs = config::default_search_dirs();
    let Ok(filters) = config::discover_all_filters(&search_dirs) else {
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
    let search_dirs = config::default_search_dirs();
    let Ok(filters) = config::discover_all_filters(&search_dirs) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    let words: Vec<&str> = command.split_whitespace().collect();

    for filter in &filters {
        if filter.matches(&words).is_some() {
            let display_name = filter
                .relative_path
                .with_extension("")
                .display()
                .to_string();
            println!(
                "{}  [{}]  command: \"{}\"",
                display_name,
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

fn main() {
    let cli = Cli::parse();
    let exit_code = match &cli.command {
        Commands::Run { command_args } => cmd_run(command_args, &cli).unwrap_or_else(|e| {
            eprintln!("[tokf] error: {e:#}");
            1
        }),
        Commands::Check { filter_path } => cmd_check(Path::new(filter_path)),
        Commands::Test {
            filter_path,
            fixture_path,
            exit_code,
        } => cmd_test(
            Path::new(filter_path),
            Path::new(fixture_path),
            *exit_code,
            &cli,
        )
        .unwrap_or_else(|e| {
            eprintln!("[tokf] error: {e:#}");
            1
        }),
        Commands::Ls => cmd_ls(cli.verbose),
        Commands::Rewrite { command } => cmd_rewrite(command),
        Commands::Which { command } => cmd_which(command, cli.verbose),
        Commands::Show { filter } => cmd_show(filter),
        Commands::Hook { action } => match action {
            HookAction::Handle => cmd_hook_handle(),
            HookAction::Install { global } => cmd_hook_install(*global),
        },
    };
    std::process::exit(exit_code);
}

fn cmd_show(filter: &str) -> i32 {
    // Normalize: strip ".toml" suffix if present
    let filter_name = filter.strip_suffix(".toml").unwrap_or(filter);

    let search_dirs = config::default_search_dirs();
    let Ok(filters) = config::discover_all_filters(&search_dirs) else {
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

    let content = if resolved.source_path.starts_with("<built-in>") {
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

fn cmd_rewrite(command: &str) -> i32 {
    let result = rewrite::rewrite(command);
    println!("{result}");
    0
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
