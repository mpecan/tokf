//! Command-line surface: the clap `Parser` and every subcommand enum.
//!
//! Split out of `main.rs` to keep that file under the size limit; `main.rs`
//! re-exports everything, so `crate::Cli` and friends still resolve.

use clap::{Parser, Subcommand};

use crate::commands::{HistoryAction, HookAction};

#[derive(Parser)]
#[command(
    name = "tokf",
    version,
    about = "Token filter — compress command output for LLM context"
)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are naturally booleans
pub struct Cli {
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
    pub otel_export: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Registry connection args shared by the CI-only `stdlib-publish` commands.
#[cfg(feature = "stdlib-publish")]
#[derive(clap::Args)]
pub struct RegistryAuthArgs {
    /// Registry base URL
    #[arg(long, env = "TOKF_REGISTRY_URL")]
    pub registry_url: String,
    /// Service token for authentication
    #[arg(long, env = "TOKF_SERVICE_TOKEN")]
    pub token: String,
}

#[derive(Subcommand)]
pub enum Commands {
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
        shell: crate::completions_cmd::ShellChoice,
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
        action: crate::cache_cmd::CacheAction,
    },
    /// View and modify tokf configuration
    Config {
        #[command(subcommand)]
        action: crate::config_cmd::ConfigAction,
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
        scope: Option<crate::verify_cmd::VerifyScope>,
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
    /// File a GitHub issue with a non-PII diagnostic snapshot (uses `gh` if available)
    Issue(crate::issue_cmd::IssueArgs),
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
        #[command(flatten)]
        auth: RegistryAuthArgs,
        /// Preview the payload without uploading
        #[arg(long)]
        dry_run: bool,
    },
    /// Backfill filter version history from git tags (CI only)
    #[cfg(feature = "stdlib-publish")]
    BackfillVersions {
        #[command(flatten)]
        auth: RegistryAuthArgs,
        /// Print computed timeline without posting to registry
        #[arg(long)]
        dry_run: bool,
    },
    /// Backfill canonical v1 hashes for legacy filters (CI only)
    #[cfg(feature = "stdlib-publish")]
    BackfillV1Hashes {
        #[command(flatten)]
        auth: RegistryAuthArgs,
        /// Rows to process per request (server caps at 500). Kept small so a
        /// round completes well inside the HTTP timeout; the command loops
        /// until the backlog is drained regardless of batch size.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Telemetry configuration and diagnostics
    Telemetry {
        #[command(subcommand)]
        action: TelemetryAction,
    },
    /// Detect filters that may be causing agent confusion (post-hoc analysis of tracking.db)
    Doctor(crate::commands::DoctorArgs),
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
    /// Detect installed AI tools and install hooks interactively
    Setup {
        /// Re-run setup even if already completed
        #[arg(long, alias = "renew")]
        refresh: bool,
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
pub enum TelemetryAction {
    /// Show telemetry configuration and connection status
    Status {
        /// Test connectivity to the OTLP endpoint
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
pub enum SkillAction {
    /// Install skill files to .claude/skills/tokf-filter/ (project-local or global)
    Install {
        /// Install globally (~/.claude/skills/) instead of project-local (.claude/skills/)
        #[arg(long)]
        global: bool,
    },
}

#[derive(Subcommand)]
pub enum AuthAction {
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
pub enum RemoteAction {
    /// Register this machine with the tokf server for remote sync
    Setup,
    /// Show remote sync registration state
    Status,
    /// Sync local usage events to the remote server
    Sync,
    /// Backfill filter hashes for past events recorded before hash tracking was added
    Backfill,
}
