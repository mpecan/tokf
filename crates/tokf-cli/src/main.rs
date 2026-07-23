mod auth_cmd;
#[cfg(feature = "stdlib-publish")]
mod backfill_cmd;
mod cache_cmd;
mod cli_args;
mod commands;
mod completions_cmd;
mod config_cmd;
mod discover_cmd;
mod doctor_cmd;
mod eject_cmd;
mod gain;
mod gain_render;
mod generic;
mod history_cmd;
mod info_cmd;
mod install_cmd;
mod issue_cmd;
mod marker;
mod output;
mod publish_cmd;
#[cfg(feature = "stdlib-publish")]
mod publish_stdlib_cmd;
mod remote_cmd;
mod resolve;
mod search_cmd;
mod setup_cmd;
mod shell;
mod show_cmd;
mod sync_cmd;
mod telemetry_cmd;
// pub(crate): accessed by install_cmd::run_verify
pub(crate) mod verify_cmd;
mod which_cmd;

use std::path::Path;

use clap::Parser;

use cli_args::{AuthAction, Cli, Commands, RemoteAction, SkillAction, TelemetryAction};
use commands::HookAction;

use tokf::telemetry;

use tokf::runtime::Runtime;
// Telemetry init + subcommand dispatch + shutdown added lines — approved to exceed 60-line limit.
#[allow(clippy::too_many_lines)]
fn main() {
    use commands::{
        cmd_apply, cmd_check, cmd_hook_handle, cmd_hook_install, cmd_ls, cmd_rewrite, cmd_run,
        cmd_skill_install, or_exit,
    };
    use which_cmd::cmd_which;

    // The one and only read of the process environment. Everything below
    // receives `&rt` rather than reaching for a global or an env var.
    let rt = Runtime::from_env();

    // `test-support` swaps in an in-memory credential store. A release build
    // with it enabled would appear to accept `tokf auth login` and persist
    // nothing, so refuse to produce one.
    #[cfg(all(feature = "test-support", not(debug_assertions)))]
    compile_error!(
        "the `test-support` feature must never be enabled in a release build: \
         it replaces the OS keyring with an in-memory mock"
    );

    #[cfg(feature = "test-support")]
    tokf::auth::credentials::use_mock_keyring();

    // Pre-clap shell mode detection.
    //
    // Task runners (make, just) invoke their shell as `$SHELL -c 'recipe_line'`.
    // When tokf is set as the shell, we intercept `-c` (and variants like `-cu`,
    // `-ec`) before clap parsing — clap would reject them as unknown flags.
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.len() >= 2 && shell::is_shell_flag(&raw_args[1]) {
        if raw_args.len() < 3 {
            eprintln!("[tokf] shell mode requires a command argument");
            std::process::exit(1);
        }
        let exit_code = if raw_args.len() == 3 {
            // String mode: task runner sends `$SHELL -c 'recipe line'`
            shell::cmd_shell(&rt, &raw_args[1], &raw_args[2])
        } else {
            // Argv mode: shim sends `tokf -c git status "$@"`
            shell::cmd_shell_argv(&rt, &raw_args[1], &raw_args[2..])
        };
        std::process::exit(exit_code);
    }

    let cli = Cli::parse();
    let reporter = telemetry::init(&rt, cli.otel_export);
    if cli.verbose {
        match reporter.endpoint_description() {
            Some(ref desc) => eprintln!("[tokf] telemetry: enabled (endpoint: {desc})"),
            None if cli.otel_export => {
                eprintln!(
                    "[tokf] telemetry: disabled or unavailable (feature not compiled, or initialization failed)"
                );
            }
            None => {}
        }
    }
    let exit_code = match &cli.command {
        Commands::Run {
            command_args,
            baseline_pipe,
            prefer_less,
        } => or_exit(cmd_run(
            &rt,
            commands::RunRequest {
                command_args,
                baseline_pipe: baseline_pipe.as_deref(),
                prefer_less: *prefer_less,
            },
            &cli,
            reporter.as_ref(),
        )),
        Commands::Completions { shell } => completions_cmd::cmd_completions(*shell),
        Commands::Check { filter_path } => cmd_check(Path::new(filter_path)),
        Commands::Apply {
            filter_path,
            fixture_path,
            exit_code,
        } => or_exit(cmd_apply(
            Path::new(filter_path),
            Path::new(fixture_path),
            *exit_code,
            &cli,
        )),
        Commands::Ls => cmd_ls(&rt, cli.verbose),
        Commands::Rewrite { command } => cmd_rewrite(&rt, command, cli.verbose),
        Commands::Which { command } => cmd_which(&rt, command, cli.verbose),
        Commands::Show { filter, hash } => show_cmd::cmd_show(&rt, filter, *hash),
        Commands::Eject { filter, global } => {
            eject_cmd::cmd_eject(&rt, filter, *global, cli.no_cache)
        }
        Commands::Hook { action } => match action {
            HookAction::Handle { format } => {
                cmd_hook_handle(&rt, format, cli.no_cache, cli.no_mask_exit_code)
            }
            HookAction::Install {
                global,
                tool,
                path,
                no_context,
            } => cmd_hook_install(&rt, *global, tool, path.as_deref(), !no_context),
        },
        Commands::Skill { action } => match action {
            SkillAction::Install { global } => cmd_skill_install(&rt, *global),
        },
        Commands::Cache { action } => cache_cmd::run_cache_action(&rt, action),
        Commands::Config { action } => config_cmd::run_config_action(&rt, action),
        Commands::Gain {
            daily,
            by_filter,
            json,
            remote,
            top,
            no_color,
        } => {
            let opts = gain::GainOpts {
                daily: *daily,
                by_filter: *by_filter,
                json: *json,
                top: *top,
                no_color: *no_color,
            };
            if *remote {
                gain::cmd_gain_remote(&rt, opts)
            } else {
                gain::cmd_gain(&rt, opts)
            }
        }
        Commands::Verify {
            filter,
            list,
            json,
            require_all,
            scope,
            safety,
        } => verify_cmd::cmd_verify(
            &rt,
            filter.as_deref(),
            *list,
            *json,
            *require_all,
            scope.as_ref(),
            *safety,
        ),
        Commands::Info { json } => info_cmd::cmd_info(&rt, *json),
        Commands::Issue(args) => issue_cmd::cmd_issue(&rt, args),
        Commands::Auth { action } => or_exit(match action {
            AuthAction::Login => auth_cmd::cmd_auth_login(&rt),
            AuthAction::Logout => auth_cmd::cmd_auth_logout(&rt),
            AuthAction::Status => auth_cmd::cmd_auth_status(&rt),
            AuthAction::DeleteAccount => auth_cmd::cmd_auth_delete_account(&rt),
        }),
        Commands::Remote { action } => or_exit(match action {
            RemoteAction::Setup => remote_cmd::cmd_remote_setup(&rt),
            RemoteAction::Status => remote_cmd::cmd_remote_status(&rt),
            RemoteAction::Sync => remote_cmd::cmd_remote_sync(&rt),
            RemoteAction::Backfill => remote_cmd::cmd_remote_backfill(&rt, cli.no_cache),
        }),
        Commands::History { action } => or_exit(history_cmd::dispatch_history(&rt, action)),
        Commands::Raw { target } => or_exit(history_cmd::dispatch_raw(&rt, target)),
        Commands::Sync { status } => or_exit(sync_cmd::cmd_sync(&rt, *status)),
        Commands::Publish {
            filter,
            dry_run,
            update_tests,
        } => publish_cmd::cmd_publish(&rt, filter, *dry_run, *update_tests),
        Commands::Search { query, limit, json } => {
            let joined = query.join(" ");
            search_cmd::cmd_search(&rt, &joined, *limit, *json)
        }
        #[cfg(feature = "stdlib-publish")]
        Commands::PublishStdlib { auth, dry_run } => {
            publish_stdlib_cmd::cmd_publish_stdlib(&rt, &auth.registry_url, &auth.token, *dry_run)
        }
        #[cfg(feature = "stdlib-publish")]
        Commands::BackfillVersions { auth, dry_run } => {
            backfill_cmd::cmd_backfill_versions(&rt, &auth.registry_url, &auth.token, *dry_run)
        }
        #[cfg(feature = "stdlib-publish")]
        Commands::BackfillV1Hashes { auth, limit } => {
            backfill_cmd::cmd_backfill_v1_hashes(&rt, &auth.registry_url, &auth.token, *limit)
        }
        Commands::Telemetry { action } => or_exit(match action {
            TelemetryAction::Status { check } => {
                telemetry_cmd::cmd_telemetry_status(&rt, *check, cli.verbose)
            }
        }),
        Commands::Discover {
            project,
            all,
            session,
            since,
            limit,
            json,
            include_filtered,
        } => or_exit(discover_cmd::cmd_discover(
            &rt,
            &discover_cmd::DiscoverOpts {
                project: project.as_deref(),
                all: *all,
                session: session.as_deref(),
                since: since.as_deref(),
                limit: *limit,
                json: *json,
                no_cache: cli.no_cache,
                include_filtered: *include_filtered,
            },
        )),
        Commands::Doctor(args) => doctor_cmd::cmd_doctor(
            &rt,
            &doctor_cmd::DoctorCliOpts {
                burst_threshold: args.burst_threshold,
                window_secs: args.window,
                project: args.project.as_deref(),
                all_projects: args.all,
                include_noise: args.include_noise,
                filter: args.filter.as_deref(),
                sort: args.sort.into(),
                json: args.json,
                no_color: args.no_color,
                no_cache: cli.no_cache,
            },
        ),
        Commands::Err {
            context,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            &rt,
            generic::GenericRun {
                command_args,
                baseline_pipe: baseline_pipe.as_deref(),
                filter_name: "_builtin/err",
            },
            &cli,
            |text, ec| generic::err::extract_errors(text, ec, *context),
        )),
        Commands::Test {
            context,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            &rt,
            generic::GenericRun {
                command_args,
                baseline_pipe: baseline_pipe.as_deref(),
                filter_name: "_builtin/test",
            },
            &cli,
            |text, ec| generic::test_run::extract_test_failures(text, ec, *context),
        )),
        Commands::Summary {
            max_lines,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            &rt,
            generic::GenericRun {
                command_args,
                baseline_pipe: baseline_pipe.as_deref(),
                filter_name: "_builtin/summary",
            },
            &cli,
            |text, ec| generic::summary::summarize(text, ec, *max_lines),
        )),
        Commands::Setup { refresh } => setup_cmd::cmd_setup(&rt, *refresh),
        Commands::Install {
            filter,
            local,
            force,
            dry_run,
            yes,
        } => install_cmd::cmd_install(
            &rt,
            install_cmd::InstallOpts {
                filter,
                local: *local,
                force: *force,
                dry_run: *dry_run,
                yes: *yes,
            },
        ),
    };
    let flushed = reporter.shutdown();
    if cli.verbose && reporter.endpoint_description().is_some() {
        if flushed {
            eprintln!("[tokf] telemetry: metrics exported");
        } else {
            eprintln!("[tokf] telemetry: export timed out — events are in local DB");
        }
    }
    std::process::exit(exit_code);
}
