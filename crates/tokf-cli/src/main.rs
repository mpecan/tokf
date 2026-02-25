mod auth_cmd;
mod cache_cmd;
mod commands;
mod eject_cmd;
mod gain;
mod history_cmd;
mod info_cmd;
mod output;
mod resolve;
mod show_cmd;
mod verify_cmd;

use std::path::Path;

use clap::{Parser, Subcommand};

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
        /// Restrict to a single filter scope (project, global, or stdlib)
        #[arg(long, value_enum)]
        scope: Option<verify_cmd::VerifyScope>,
    },
    /// Show system paths, database locations, and filter counts
    Info {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Authenticate with the tokf server
    Auth {
        #[command(subcommand)]
        action: AuthAction,
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

// R6: Rename Opencode → OpenCode; use #[value(name = "opencode")] to keep CLI arg as "opencode".
#[derive(clap::ValueEnum, Clone, Default, Debug)]
pub(crate) enum HookTool {
    #[default]
    ClaudeCode,
    #[value(name = "opencode")]
    OpenCode,
    #[value(name = "codex")]
    Codex,
}

#[derive(Subcommand)]
enum HookAction {
    /// Handle a `PreToolUse` hook invocation (reads JSON from stdin)
    Handle,
    /// Install the integration for the target tool
    Install {
        /// Install globally instead of project-local
        #[arg(long)]
        global: bool,
        /// Target tool to install hook for (default: claude-code)
        #[arg(long, value_enum, default_value_t = HookTool::ClaudeCode)]
        tool: HookTool,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Log in via GitHub device flow
    Login,
    /// Log out and remove stored credentials
    Logout,
    /// Show current authentication status
    Status,
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
        /// Print only the raw captured output (no metadata, no filtered output)
        #[arg(long)]
        raw: bool,
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

#[allow(clippy::too_many_lines)]
fn main() {
    use commands::{
        cmd_check, cmd_hook_handle, cmd_hook_install, cmd_ls, cmd_rewrite, cmd_run,
        cmd_skill_install, cmd_test, cmd_which, or_exit,
    };

    let cli = Cli::parse();
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
        )),
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
        Commands::Show { filter, hash } => show_cmd::cmd_show(filter, *hash),
        Commands::Eject { filter, global } => eject_cmd::cmd_eject(filter, *global, cli.no_cache),
        Commands::Hook { action } => match action {
            HookAction::Handle => cmd_hook_handle(),
            HookAction::Install { global, tool } => cmd_hook_install(*global, tool),
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
            scope,
        } => verify_cmd::cmd_verify(
            filter.as_deref(),
            *list,
            *json,
            *require_all,
            scope.as_ref(),
        ),
        Commands::Info { json } => info_cmd::cmd_info(*json),
        Commands::Auth { action } => or_exit(match action {
            AuthAction::Login => auth_cmd::cmd_auth_login(),
            AuthAction::Logout => auth_cmd::cmd_auth_logout(),
            AuthAction::Status => auth_cmd::cmd_auth_status(),
        }),
        Commands::History { action } => or_exit(match action {
            HistoryAction::List { limit, all } => history_cmd::cmd_history_list(*limit, *all),
            HistoryAction::Show { id, raw } => history_cmd::cmd_history_show(*id, *raw),
            HistoryAction::Search { query, limit, all } => {
                history_cmd::cmd_history_search(query, *limit, *all)
            }
            HistoryAction::Clear { all } => history_cmd::cmd_history_clear(*all),
        }),
    };
    std::process::exit(exit_code);
}
