pub mod err;
pub mod patterns;
pub mod summary;
pub mod test_run;

use tokf::baseline;
use tokf::history;
use tokf::runner;

use crate::Cli;
use crate::resolve;

/// Shared execution + tracking for generic fallback commands.
///
/// 1. Execute command
/// 2. Compute baseline (if `--baseline-pipe`)
/// 3. Apply `filter_fn` to combined output
/// 4. Handle exit-code masking
/// 5. Record tracking + history
/// 6. Print output with compression indicator
pub fn cmd_generic_run(
    command_args: &[String],
    baseline_pipe: Option<&str>,
    filter_name: &str,
    cli: &Cli,
    filter_fn: impl FnOnce(&str, i32) -> String,
) -> anyhow::Result<i32> {
    let cmd_result = runner::execute(&command_args[0], &command_args[1..])?;

    let raw_bytes = cmd_result.combined.len();
    let input_bytes = match baseline_pipe {
        Some(pipe_cmd) => baseline::compute(&cmd_result.combined, pipe_cmd),
        None => raw_bytes,
    };

    let start = std::time::Instant::now();
    let filtered = filter_fn(&cmd_result.combined, cmd_result.exit_code);
    let elapsed = start.elapsed();

    if cli.timing {
        eprintln!(
            "[tokf] {filter_name} took {:.1}ms",
            elapsed.as_secs_f64() * 1000.0
        );
    }

    let output_bytes = filtered.len();

    let mask = !cli.no_mask_exit_code && cmd_result.exit_code != 0;
    if mask {
        println!("Error: Exit code {}", cmd_result.exit_code);
    }

    let command_str = command_args.join(" ");

    // Load output config for compression indicator
    let output_cfg = {
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_root = history::project_root_for(&cwd);
        history::OutputConfig::load(Some(&project_root))
    };

    if !filtered.is_empty() {
        if output_cfg.show_indicator {
            println!("🗜️ {filtered}");
        } else {
            println!("{filtered}");
        }
    }

    // Record tracking
    resolve::record_run(
        command_args,
        Some(filter_name),
        None,
        input_bytes,
        output_bytes,
        raw_bytes,
        elapsed.as_millis(),
        cmd_result.exit_code,
        false,
    );
    resolve::try_auto_sync();

    // Record history
    history::try_record(
        &command_str,
        filter_name,
        &cmd_result.combined,
        &filtered,
        cmd_result.exit_code,
    );

    if cli.no_mask_exit_code {
        Ok(cmd_result.exit_code)
    } else {
        Ok(0)
    }
}
