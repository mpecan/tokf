use std::path::Path;

use clap::{Parser, Subcommand};

use tokf::config;
use tokf::config::types::FilterConfig;
use tokf::filter;
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
}

/// Try progressively shorter prefixes of `command_args` to find a matching filter.
/// Returns `(Option<FilterConfig>, words_consumed)`.
fn find_filter(
    command_args: &[String],
    verbose: bool,
) -> anyhow::Result<(Option<FilterConfig>, usize)> {
    let search_dirs = config::default_search_dirs();
    let words: Vec<&str> = command_args.iter().map(String::as_str).collect();

    for len in (1..=words.len()).rev() {
        let candidate = &words[..len];
        let filter_name = config::command_to_filter_name(candidate);

        for dir in &search_dirs {
            let path = dir.join(&filter_name);
            if verbose {
                eprintln!("[tokf] trying {}", path.display());
            }
            if let Some(cfg) = config::try_load_filter(&path)? {
                if verbose {
                    eprintln!("[tokf] matched {filter_name} in {}", dir.display());
                }
                return Ok((Some(cfg), len));
            }
        }
    }

    if verbose {
        eprintln!("[tokf] no filter found, passing through");
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
                cfg.command
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
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for dir in &search_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };

        let mut toml_files: Vec<_> = entries
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
            .collect();
        toml_files.sort_by_key(std::fs::DirEntry::file_name);

        for entry in toml_files {
            let filename = entry.file_name().to_string_lossy().to_string();
            let filter_name = filename.trim_end_matches(".toml");

            if !seen.insert(filter_name.to_string()) {
                if verbose {
                    eprintln!("[tokf] shadowed: {filename} in {}", dir.display());
                }
                continue;
            }

            match config::try_load_filter(&entry.path()) {
                Ok(Some(cfg)) => {
                    println!("{filter_name}  \u{2192}  {}", cfg.command);
                    if verbose {
                        eprintln!("[tokf]   source: {}", dir.display());
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    println!("{filter_name}  (invalid)");
                }
            }
        }
    }

    0
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
    };
    std::process::exit(exit_code);
}
