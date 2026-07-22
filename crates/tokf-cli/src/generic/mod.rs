pub mod err;
pub mod patterns;
pub mod summary;
pub mod test_run;

use tokf::baseline;
use tokf::history;
use tokf::runner;

use crate::Cli;
use crate::marker;
use crate::resolve;

use tokf::runtime::Runtime;

/// The command a built-in filter subcommand should run, and how to label it.
#[derive(Clone, Copy)]
pub struct GenericRun<'a> {
    pub command_args: &'a [String],
    pub baseline_pipe: Option<&'a str>,
    pub filter_name: &'a str,
}

/// Shared execution + tracking for generic fallback commands.
///
/// 1. Execute command
/// 2. Compute baseline (if `--baseline-pipe`)
/// 3. Apply `filter_fn` to combined output
/// 4. Handle exit-code masking
/// 5. Record tracking + history
/// 6. Print output with compression indicator
pub fn cmd_generic_run(
    rt: &Runtime,
    run: GenericRun<'_>,
    cli: &Cli,
    filter_fn: impl FnOnce(&str, i32) -> String,
) -> anyhow::Result<i32> {
    let GenericRun {
        command_args,
        baseline_pipe,
        filter_name,
    } = run;
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

    // Record history first: the entry ID feeds the recovery marker printed below.
    let history_id = history::try_record(
        rt,
        &history::RecordedRun {
            command: &command_str,
            // The generic path executes `command_args` verbatim — no `run`
            // override exists, so there is no substitution to record.
            executed_command: None,
            filter_name,
            raw_output: &cmd_result.combined,
            filtered_output: &filtered,
            exit_code: cmd_result.exit_code,
        },
    );

    let render_cfg = marker::load_render_config(rt);
    if !filtered.is_empty() {
        marker::print_with_indicator(&filtered, &render_cfg, history_id);
    }

    // Record tracking
    resolve::record_run(
        rt,
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
    resolve::try_auto_sync(rt);

    if cli.no_mask_exit_code {
        Ok(cmd_result.exit_code)
    } else {
        Ok(0)
    }
}
