mod auth_cmd;
#[cfg(feature = "stdlib-publish")]
mod backfill_cmd;
mod cache_cmd;
mod commands;
mod completions_cmd;
mod config_cmd;
mod discover_cmd;
mod eject_cmd;
mod gain;
mod gain_render;
mod generic;
mod history_cmd;
mod info_cmd;
mod install_cmd;
mod output;
mod publish_cmd;
#[cfg(feature = "stdlib-publish")]
mod publish_stdlib_cmd;
mod remote_cmd;
mod resolve;
mod search_cmd;
mod shell;
mod show_cmd;
mod sync_cmd;
mod telemetry_cmd;
// pub(crate): accessed by install_cmd::run_verify
pub(crate) mod verify_cmd;

use std::path::Path;

use clap::{Parser, Subcommand};

use commands::{HistoryAction, HookAction};

use tokf::telemetry;
#[derive(Parser)]
#[command(
    name = "tokf",
    version,
    about = "Token filter — compress command output for LLM context"
)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are naturally booleans
pub(crate) struct Cli {
    /// Show how long filtering took
    #[arg(long, global = true)]
    pub timing: bool,

    /// Skip filtering, pass output through raw
    #[arg(long, global = true)]
    pub no_filter: bool,

    /// Show filter resolution details
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Bypass the binary config cache for this invocation
    #[arg(long, global = true)]
    pub no_cache: bool,

    /// Disable exit-code masking. By default tokf exits 0 and prepends
    /// "Error: Exit code N" to output when the underlying command fails.
    /// This flag restores real exit-code propagation.
    #[arg(long, global = true)]
    pub no_mask_exit_code: bool,

    /// Preserve ANSI color codes in filtered output. Internally strips ANSI
    /// for pattern matching but restores original colored lines in the result.
    /// Note: this is not `--color=always/never/auto` — it controls passthrough
    /// of the child command's existing ANSI codes through the filter pipeline.
    #[arg(long, global = true, env = "TOKF_PRESERVE_COLOR")]
    pub preserve_color: bool,

    /// Export metrics via OpenTelemetry OTLP (requires --features otel)
    #[arg(long, global = true)]
    otel_export: bool,

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
        /// Use whichever output is smaller: filtered or piped (no-op without --baseline-pipe)
        #[arg(long)]
        prefer_less: bool,
        #[arg(trailing_var_arg = true, required = true)]
        command_args: Vec<String>,
    },
    /// Generate shell completion scripts
    Completions {
        /// Target shell (bash, zsh, fish, powershell, elvish, nushell)
        shell: completions_cmd::ShellChoice,
    },
    /// Validate a filter TOML file
    Check {
        /// Path to the filter file
        filter_path: String,
    },
    /// Apply a filter to a fixture file (formerly `test`)
    #[command(alias = "test-filter")]
    Apply {
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
        /// Print the SHA-256 content hash of the filter (for identity verification or change detection)
        #[arg(long)]
        hash: bool,
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
    /// View and modify tokf configuration
    Config {
        #[command(subcommand)]
        action: config_cmd::ConfigAction,
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
        /// Query remote server stats instead of local database
        #[arg(long)]
        remote: bool,
        /// Number of top filters to show in the summary view (default: 10)
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Disable colored output (also respects the `NO_COLOR` environment variable)
        #[arg(long)]
        no_color: bool,
    },
    /// Manage filtered output history
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Print raw (unfiltered) output — `tokf raw last` or `tokf raw <id>`
    Raw {
        /// "last" for most recent, or a numeric entry ID
        target: String,
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
        /// Restrict to a single filter scope (project, global, or stdlib)
        #[arg(long, value_enum)]
        scope: Option<verify_cmd::VerifyScope>,
        /// Run safety checks (detect prompt injection, shell injection, hidden unicode)
        #[arg(long)]
        safety: bool,
    },
    /// Show system paths, database locations, and filter counts
    Info {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Authenticate with the tokf server (credentials stored in OS keyring)
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Register this machine and manage remote sync settings
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },
    /// Publish a local filter to the community registry
    Publish {
        /// Filter name to publish (e.g. "git/my-filter")
        filter: String,
        /// Preview what would be published without uploading
        #[arg(long)]
        dry_run: bool,
        /// Replace the test suite for an already-published filter (author-only)
        #[arg(long)]
        update_tests: bool,
    },
    /// Search the community filter registry
    Search {
        /// Maximum number of results to return
        #[arg(long, short = 'n', default_value_t = 20)]
        limit: usize,
        /// Output results as JSON
        #[arg(long)]
        json: bool,
        /// Search query (matches command pattern)
        #[arg(trailing_var_arg = true, required = true)]
        query: Vec<String>,
    },
    /// Sync local usage data to the remote server
    Sync {
        /// Show last sync time and count of pending events
        #[arg(long)]
        status: bool,
    },
    /// Publish all stdlib filters to the registry (CI only)
    #[cfg(feature = "stdlib-publish")]
    PublishStdlib {
        /// Registry base URL
        #[arg(long, env = "TOKF_REGISTRY_URL")]
        registry_url: String,
        /// Service token for authentication
        #[arg(long, env = "TOKF_SERVICE_TOKEN")]
        token: String,
        /// Preview the payload without uploading
        #[arg(long)]
        dry_run: bool,
    },
    /// Backfill filter version history from git tags (CI only)
    #[cfg(feature = "stdlib-publish")]
    BackfillVersions {
        /// Registry base URL
        #[arg(long, env = "TOKF_REGISTRY_URL")]
        registry_url: String,
        /// Service token for authentication
        #[arg(long, env = "TOKF_SERVICE_TOKEN")]
        token: String,
        /// Print computed timeline without posting to registry
        #[arg(long)]
        dry_run: bool,
    },
    /// Telemetry configuration and diagnostics
    Telemetry {
        #[command(subcommand)]
        action: TelemetryAction,
    },
    /// Find missed token savings in Claude Code sessions
    Discover {
        /// Project path to scan (defaults to current directory)
        #[arg(long, conflicts_with_all = ["all", "session"])]
        project: Option<String>,
        /// Scan all projects
        #[arg(long, conflicts_with = "session")]
        all: bool,
        /// Path to a single session file
        #[arg(long)]
        session: Option<String>,
        /// Only scan sessions modified within this window (e.g. 7d, 24h)
        #[arg(long)]
        since: Option<String>,
        /// Maximum number of results to show (0 = unlimited, default: 20)
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Also show commands that already have a matching filter
        #[arg(long)]
        include_filtered: bool,
    },
    /// Run a command and show only errors/warnings
    Err {
        /// Lines of context around each error
        #[arg(short = 'C', long, default_value_t = 3)]
        context: usize,
        /// Pipe command for fair baseline accounting
        #[arg(long)]
        baseline_pipe: Option<String>,
        #[arg(trailing_var_arg = true, required = true)]
        command_args: Vec<String>,
    },
    /// Run a test command and show only failures
    Test {
        /// Lines of context around each failure
        #[arg(short = 'C', long, default_value_t = 5)]
        context: usize,
        /// Pipe command for fair baseline accounting
        #[arg(long)]
        baseline_pipe: Option<String>,
        #[arg(trailing_var_arg = true, required = true)]
        command_args: Vec<String>,
    },
    /// Run a command and produce a heuristic summary
    Summary {
        /// Maximum lines in the summary output
        #[arg(long, default_value_t = 30)]
        max_lines: usize,
        /// Pipe command for fair baseline accounting
        #[arg(long)]
        baseline_pipe: Option<String>,
        #[arg(trailing_var_arg = true, required = true)]
        command_args: Vec<String>,
    },
    /// Install a filter from the community registry
    Install {
        /// Filter hash (64 hex chars) or command pattern to search for
        filter: String,
        /// Install to project-local .tokf/filters/ instead of global config
        #[arg(long)]
        local: bool,
        /// Overwrite an existing filter at the same path
        #[arg(long)]
        force: bool,
        /// Preview what would be installed without writing files
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompts (Lua filters still emit an audit warning)
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum TelemetryAction {
    /// Show telemetry configuration and connection status
    Status {
        /// Test connectivity to the OTLP endpoint
        #[arg(long)]
        check: bool,
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
enum AuthAction {
    /// Log in via GitHub device flow (opens browser, stores token in OS keyring)
    Login,
    /// Log out and remove stored credentials (keyring token + config metadata)
    Logout,
    /// Show current authentication status (username, server URL)
    Status,
    /// Permanently delete your account (requires confirmation)
    DeleteAccount,
}

#[derive(Subcommand)]
enum RemoteAction {
    /// Register this machine with the tokf server for remote sync
    Setup,
    /// Show remote sync registration state
    Status,
    /// Sync local usage events to the remote server
    Sync,
    /// Backfill filter hashes for past events recorded before hash tracking was added
    Backfill,
}

// Telemetry init + subcommand dispatch + shutdown added lines — approved to exceed 60-line limit.
#[allow(clippy::too_many_lines)]
fn main() {
    use commands::{
        cmd_apply, cmd_check, cmd_hook_handle, cmd_hook_install, cmd_ls, cmd_rewrite, cmd_run,
        cmd_skill_install, cmd_which, or_exit,
    };

    tokf::paths::init_from_env();

    #[cfg(feature = "test-keyring")]
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
            shell::cmd_shell(&raw_args[1], &raw_args[2])
        } else {
            // Argv mode: shim sends `tokf -c git status "$@"`
            shell::cmd_shell_argv(&raw_args[1], &raw_args[2..])
        };
        std::process::exit(exit_code);
    }

    let cli = Cli::parse();
    let reporter = telemetry::init(cli.otel_export);
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
            command_args,
            baseline_pipe.as_deref(),
            *prefer_less,
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
        Commands::Ls => cmd_ls(cli.verbose),
        Commands::Rewrite { command } => cmd_rewrite(command, cli.verbose),
        Commands::Which { command } => cmd_which(command, cli.verbose),
        Commands::Show { filter, hash } => show_cmd::cmd_show(filter, *hash),
        Commands::Eject { filter, global } => eject_cmd::cmd_eject(filter, *global, cli.no_cache),
        Commands::Hook { action } => match action {
            HookAction::Handle { format } => cmd_hook_handle(format),
            HookAction::Install {
                global,
                tool,
                path,
                no_context,
            } => cmd_hook_install(*global, tool, path.as_deref(), !no_context),
        },
        Commands::Skill { action } => match action {
            SkillAction::Install { global } => cmd_skill_install(*global),
        },
        Commands::Cache { action } => cache_cmd::run_cache_action(action),
        Commands::Config { action } => config_cmd::run_config_action(action),
        Commands::Gain {
            daily,
            by_filter,
            json,
            remote,
            top,
            no_color,
        } => {
            if *remote {
                gain::cmd_gain_remote(*daily, *by_filter, *json, *top, *no_color)
            } else {
                gain::cmd_gain(*daily, *by_filter, *json, *top, *no_color)
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
            filter.as_deref(),
            *list,
            *json,
            *require_all,
            scope.as_ref(),
            *safety,
        ),
        Commands::Info { json } => info_cmd::cmd_info(*json),
        Commands::Auth { action } => or_exit(match action {
            AuthAction::Login => auth_cmd::cmd_auth_login(),
            AuthAction::Logout => auth_cmd::cmd_auth_logout(),
            AuthAction::Status => auth_cmd::cmd_auth_status(),
            AuthAction::DeleteAccount => auth_cmd::cmd_auth_delete_account(),
        }),
        Commands::Remote { action } => or_exit(match action {
            RemoteAction::Setup => remote_cmd::cmd_remote_setup(),
            RemoteAction::Status => remote_cmd::cmd_remote_status(),
            RemoteAction::Sync => remote_cmd::cmd_remote_sync(),
            RemoteAction::Backfill => remote_cmd::cmd_remote_backfill(cli.no_cache),
        }),
        Commands::History { action } => or_exit(match action {
            HistoryAction::List { limit, all } => history_cmd::cmd_history_list(*limit, *all),
            HistoryAction::Show { id, raw } => history_cmd::cmd_history_show(*id, *raw),
            HistoryAction::Last { raw, all } => history_cmd::cmd_history_last(*raw, *all),
            HistoryAction::Search { query, limit, all } => {
                history_cmd::cmd_history_search(query, *limit, *all)
            }
            HistoryAction::Clear { all } => history_cmd::cmd_history_clear(*all),
        }),
        Commands::Raw { target } => or_exit(if target == "last" {
            history_cmd::cmd_history_last(true, false)
        } else if let Ok(id) = target.parse::<i64>() {
            history_cmd::cmd_history_show(id, true)
        } else {
            eprintln!("[tokf] expected `last` or a numeric ID, got: {target}");
            Ok(1)
        }),
        Commands::Sync { status } => or_exit(sync_cmd::cmd_sync(*status)),
        Commands::Publish {
            filter,
            dry_run,
            update_tests,
        } => publish_cmd::cmd_publish(filter, *dry_run, *update_tests),
        Commands::Search { query, limit, json } => {
            let joined = query.join(" ");
            search_cmd::cmd_search(&joined, *limit, *json)
        }
        #[cfg(feature = "stdlib-publish")]
        Commands::PublishStdlib {
            registry_url,
            token,
            dry_run,
        } => publish_stdlib_cmd::cmd_publish_stdlib(registry_url, token, *dry_run),
        #[cfg(feature = "stdlib-publish")]
        Commands::BackfillVersions {
            registry_url,
            token,
            dry_run,
        } => backfill_cmd::cmd_backfill_versions(registry_url, token, *dry_run),
        Commands::Telemetry { action } => or_exit(match action {
            TelemetryAction::Status { check } => {
                telemetry_cmd::cmd_telemetry_status(*check, cli.verbose)
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
        } => or_exit(discover_cmd::cmd_discover(&discover_cmd::DiscoverOpts {
            project: project.as_deref(),
            all: *all,
            session: session.as_deref(),
            since: since.as_deref(),
            limit: *limit,
            json: *json,
            no_cache: cli.no_cache,
            include_filtered: *include_filtered,
        })),
        Commands::Err {
            context,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            command_args,
            baseline_pipe.as_deref(),
            "_builtin/err",
            &cli,
            |text, ec| generic::err::extract_errors(text, ec, *context),
        )),
        Commands::Test {
            context,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            command_args,
            baseline_pipe.as_deref(),
            "_builtin/test",
            &cli,
            |text, ec| generic::test_run::extract_test_failures(text, ec, *context),
        )),
        Commands::Summary {
            max_lines,
            baseline_pipe,
            command_args,
        } => or_exit(generic::cmd_generic_run(
            command_args,
            baseline_pipe.as_deref(),
            "_builtin/summary",
            &cli,
            |text, ec| generic::summary::summarize(text, ec, *max_lines),
        )),
        Commands::Install {
            filter,
            local,
            force,
            dry_run,
            yes,
        } => install_cmd::cmd_install(filter, *local, *force, *dry_run, *yes),
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
