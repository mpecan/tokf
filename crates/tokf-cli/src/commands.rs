use std::path::Path;

use tokf::baseline;
use tokf::config;
use tokf::filter;
use tokf::history;
use tokf::hook;
use tokf::rewrite;
use tokf::runner;
use tokf::skill;

use crate::resolve;
use crate::{Cli, HookTool};

pub fn or_exit(r: anyhow::Result<i32>) -> i32 {
    r.unwrap_or_else(|e| {
        eprintln!("[tokf] error: {e:#}");
        1
    })
}

// NOTE: cmd_run integrates command resolution, execution, output rendering, tracking,
// and history recording. Splitting would require threading 5+ values through helpers.
// Approved to exceed the 60-line limit.
#[allow(clippy::too_many_lines)]
pub fn cmd_run(
    command_args: &[String],
    baseline_pipe: Option<&str>,
    prefer_less: bool,
    cli: &Cli,
) -> anyhow::Result<i32> {
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
        if prefer_less && cli.verbose {
            eprintln!("[tokf] --prefer-less has no effect: no matching filter found");
        }
        let raw_len = cmd_result.combined.len();
        let input_bytes = match baseline_pipe {
            Some(pipe_cmd) => baseline::compute(&cmd_result.combined, pipe_cmd),
            None => raw_len,
        };
        let mask = !cli.no_mask_exit_code && cmd_result.exit_code != 0;
        if mask {
            println!("Error: Exit code {}", cmd_result.exit_code);
        }
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
            false,
        );
        if cli.no_mask_exit_code {
            return Ok(cmd_result.exit_code);
        }
        return Ok(0);
    };

    // Phase B: resolve deferred output-pattern variants using the already-discovered
    // filter list (no second discovery call needed).
    let cfg = resolve::resolve_phase_b(filter_match, &cmd_result.combined, cli.verbose);

    // Compute piped output once: when prefer_less is active we need the full text
    // for comparison, otherwise just the byte count for tracking.
    let (input_bytes, piped_text) = match baseline_pipe {
        Some(pipe_cmd) if prefer_less => {
            let text = baseline::compute_output(&cmd_result.combined, pipe_cmd);
            let bytes = text.as_ref().map_or(cmd_result.combined.len(), String::len);
            (bytes, text)
        }
        Some(pipe_cmd) => (baseline::compute(&cmd_result.combined, pipe_cmd), None),
        None => (cmd_result.combined.len(), None),
    };

    let start = std::time::Instant::now();
    let filter_opts = filter::FilterOptions {
        preserve_color: cli.preserve_color,
    };
    let filtered = filter::apply(&cfg, &cmd_result, &remaining_args, &filter_opts);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    // --prefer-less: compare filtered output with cached piped output, use whichever is smaller.
    let (final_output, output_bytes, pipe_override) =
        if let Some(piped) = piped_text.filter(|t| t.len() < filtered.output.len()) {
            if cli.verbose {
                eprintln!(
                    "[tokf] prefer-less: pipe output ({} bytes) < filtered ({} bytes), using pipe",
                    piped.len(),
                    filtered.output.len()
                );
            }
            let len = piped.len();
            (piped, len, true)
        } else {
            let len = filtered.output.len();
            (filtered.output, len, false)
        };

    let filter_name = cfg.command.first();
    let command_str = command_args.join(" ");

    resolve::record_run(
        command_args,
        Some(filter_name),
        input_bytes,
        output_bytes,
        elapsed.as_millis(),
        cmd_result.exit_code,
        pipe_override,
    );

    // Detect whether to show the history hint:
    //   - filter author opted in via `show_history_hint = true`, or
    //   - the same command was re-run (LLM confusion signal: it didn't act on
    //     the previous filtered output and is asking again).
    // Check the DB before recording so we compare against the *previous* run.
    let show_hint = cfg.show_history_hint || history::try_was_recently_run(&command_str);

    let history_id = history::try_record(
        &command_str,
        filter_name,
        &cmd_result.combined,
        &final_output,
        cmd_result.exit_code,
    );

    let mask = !cli.no_mask_exit_code && cmd_result.exit_code != 0;
    if mask {
        println!("Error: Exit code {}", cmd_result.exit_code);
    }
    if !final_output.is_empty() {
        println!("{final_output}");
    }

    if show_hint && let Some(id) = history_id {
        println!(
            "[tokf] output filtered — to see what was omitted: `tokf history show --raw {id}`"
        );
    }

    if cli.no_mask_exit_code {
        Ok(cmd_result.exit_code)
    } else {
        Ok(0)
    }
}

pub fn cmd_check(filter_path: &Path) -> i32 {
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

pub fn cmd_test(
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
    let filter_opts = filter::FilterOptions {
        preserve_color: cli.preserve_color,
    };
    let filtered = filter::apply(&cfg, &cmd_result, &[], &filter_opts);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!("[tokf] filter took {:.1}ms", elapsed.as_secs_f64() * 1000.0);
    }

    // tokf test always writes to stdout — it's a debugging tool that always
    // exits 0, not a hook-invoked path subject to the stderr-on-failure routing.
    if !filtered.output.is_empty() {
        println!("{}", filtered.output);
    }

    Ok(0)
}

// Note: cmd_ls and cmd_which always use the cache. The --no-cache flag
// only affects `tokf run`. Pass --no-cache to `tokf run` if you need uncached resolution.
pub fn cmd_ls(verbose: bool) -> i32 {
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

pub fn cmd_which(command: &str, verbose: bool) -> i32 {
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

pub fn cmd_rewrite(command: &str, verbose: bool) -> i32 {
    let result = rewrite::rewrite(command, verbose);
    println!("{result}");
    0
}

pub fn cmd_skill_install(global: bool) -> i32 {
    match skill::install(global) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

pub fn cmd_hook_handle() -> i32 {
    hook::handle();
    0
}

pub fn cmd_hook_install(global: bool, tool: &HookTool) -> i32 {
    let result = match tool {
        HookTool::ClaudeCode => hook::install(global),
        // R6: Updated variant name from Opencode to OpenCode.
        HookTool::OpenCode => hook::opencode::install(global),
        HookTool::Codex => hook::codex::install(global),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            // R5: Standardize eprintln prefix to [tokf].
            eprintln!("[tokf] hook install failed: {e:#}");
            1
        }
    }
}
