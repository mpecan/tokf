//! `tokf issue` — gather a non-PII diagnostic snapshot, show it to the user,
//! and submit it via `gh` (or fall back to a printable markdown document).
//!
//! The body is **always** previewed on stderr before any submission. PII is
//! filtered at the rendering layer (home prefix → `~`); the file additionally
//! avoids collecting hostname, username, machine UUID, or auth tokens.

use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::path::Path;

use anyhow::{Context, Result};

use crate::info_cmd::{self, InfoOutput, WriteAccess};

const DEFAULT_REPO: &str = "mpecan/tokf";

/// GitHub's `issues/new?...` URL has practical length limits; stay well under
/// browser/server caps. Above this, fall back to copy/paste guidance.
const ISSUES_NEW_URL_BUDGET: usize = 8000;

/// Args for `tokf issue`. Defined here (not inline in `main.rs`) so the
/// per-flag doc comments don't push `main.rs` over the 700-line hard limit;
/// `#[command(flatten)]` in `Commands::Issue` inlines them into the
/// subcommand. Mirrors the pattern used by `commands::DoctorArgs`.
#[derive(clap::Args, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // CLI flags are naturally booleans.
pub struct IssueArgs {
    /// Issue title (skip the interactive prompt)
    #[arg(long)]
    pub title: Option<String>,
    /// Issue description body (mutually exclusive with --body-from)
    #[arg(long, conflicts_with = "body_from")]
    pub body: Option<String>,
    /// Read body from file path, or `-` for stdin
    #[arg(long, value_name = "FILE")]
    pub body_from: Option<String>,
    /// Print the markdown to stdout instead of submitting via gh
    #[arg(long)]
    pub print: bool,
    /// Skip the confirmation prompt before submitting
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Override the destination repository
    #[arg(long, value_name = "OWNER/REPO", default_value = DEFAULT_REPO)]
    pub repo: String,
    /// Include the names of every loaded filter in the report (off by default)
    #[arg(long)]
    pub include_filters: bool,
}

#[derive(Debug, Clone)]
struct EnvSnapshot {
    os: &'static str,
    arch: &'static str,
    shell: Option<String>,
    has_gh: bool,
    has_git: bool,
}

struct FilterNames {
    local: Vec<String>,
    user: Vec<String>,
    builtin: Vec<String>,
}

pub fn cmd_issue(opts: &IssueArgs) -> i32 {
    if opts.body.is_some() && opts.body_from.is_some() {
        eprintln!("[tokf] --body and --body-from are mutually exclusive");
        return 2;
    }

    let prepared = match prepare_issue(opts) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[tokf] {e:#}");
            return 1;
        }
    };

    print_preview(&prepared);
    dispatch(opts, &prepared)
}

struct PreparedIssue {
    title: String,
    markdown: String,
}

fn prepare_issue(opts: &IssueArgs) -> Result<PreparedIssue> {
    let title = resolve_title(opts)?;
    let user_body = resolve_body(opts)?;

    let search_dirs = tokf::config::default_search_dirs();
    let discovered_filters = match tokf::config::discover_all_filters(&search_dirs) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("[tokf] error discovering filters: {e:#}");
            None
        }
    };
    let info = info_cmd::collect_info_with_filters(&search_dirs, discovered_filters.as_deref());
    let env = collect_env();

    let include_filters = opts.include_filters || should_prompt_for_filters(opts, &info);
    let filter_names = include_filters
        .then(|| discovered_filters.as_deref().map(collect_filter_names))
        .flatten();

    let home = dirs::home_dir();
    let inputs = MarkdownInputs {
        user_body: &user_body,
        info: &info,
        env: &env,
        filters: filter_names.as_ref(),
        home: home.as_deref(),
    };
    let markdown = render_markdown(&inputs);
    Ok(PreparedIssue { title, markdown })
}

fn print_preview(p: &PreparedIssue) {
    eprintln!("\n--- ISSUE PREVIEW ---");
    eprintln!("title: {}", p.title);
    eprintln!();
    eprintln!("{}", p.markdown);
    eprintln!("--- END PREVIEW ---\n");
}

fn dispatch(opts: &IssueArgs, p: &PreparedIssue) -> i32 {
    if opts.print {
        println!("{}", p.markdown);
        return 0;
    }
    if which::which("gh").is_err() {
        eprintln!("[tokf] gh not found on PATH — falling back to printable output.");
        fallback_print(&opts.repo, &p.title, &p.markdown);
        return 0;
    }
    if !opts.yes && !confirm_submit(&opts.repo) {
        eprintln!("[tokf] not submitting — printing markdown for copy/paste.");
        fallback_print(&opts.repo, &p.title, &p.markdown);
        return 0;
    }
    match submit_via_gh(&opts.repo, &p.title, &p.markdown) {
        Ok(url) => {
            if !url.is_empty() {
                println!("{url}");
            }
            0
        }
        Err(e) => {
            eprintln!("[tokf] gh submission failed: {e:#}");
            eprintln!("[tokf] falling back to printable output:");
            fallback_print(&opts.repo, &p.title, &p.markdown);
            1
        }
    }
}

fn resolve_title(opts: &IssueArgs) -> Result<String> {
    if let Some(t) = &opts.title {
        let t = t.trim().to_string();
        if t.is_empty() {
            anyhow::bail!("--title cannot be empty");
        }
        return Ok(t);
    }
    let raw: String = dialoguer::Input::new()
        .with_prompt("Issue title")
        .interact_text()
        .context("reading title")?;
    let t = raw.trim().to_string();
    if t.is_empty() {
        anyhow::bail!("title cannot be empty");
    }
    Ok(t)
}

fn resolve_body(opts: &IssueArgs) -> Result<String> {
    if let Some(b) = &opts.body {
        return Ok(b.clone());
    }
    if let Some(p) = &opts.body_from {
        if p == "-" {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading body from stdin")?;
            return Ok(s);
        }
        return std::fs::read_to_string(p).with_context(|| format!("reading body from {p}"));
    }
    if let Some(b) = read_body_via_editor()? {
        return Ok(b);
    }
    let raw: String = dialoguer::Input::new()
        .with_prompt("Describe the issue (single line; pass --body-from for longer text)")
        .allow_empty(true)
        .interact_text()
        .context("reading body")?;
    Ok(raw)
}

/// Open `$EDITOR` (or `$VISUAL`) on a temp file with comment guidance.
/// Returns `Ok(None)` when no editor is configured. Strips `#`-prefixed
/// lines from the result.
fn read_body_via_editor() -> Result<Option<String>> {
    let editor = std::env::var("EDITOR")
        .ok()
        .or_else(|| std::env::var("VISUAL").ok())
        .filter(|s| !s.trim().is_empty());
    let Some(editor) = editor else {
        return Ok(None);
    };
    let tmp = tempfile::Builder::new()
        .prefix("tokf-issue-")
        .suffix(".md")
        .tempfile()
        .context("creating temp file for editor")?;
    std::fs::write(
        tmp.path(),
        "# Describe the issue. Lines starting with '#' are removed.\n\
         # Save and quit when done.\n\n",
    )
    .context("seeding temp file")?;
    let status = std::process::Command::new(&editor)
        .arg(tmp.path())
        .status()
        .with_context(|| format!("running editor: {editor}"))?;
    if !status.success() {
        anyhow::bail!("editor exited with non-zero status: {status}");
    }
    let raw = std::fs::read_to_string(tmp.path()).context("reading edited body")?;
    let cleaned: String = raw
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(cleaned.trim().to_string()))
}

fn confirm_submit(repo: &str) -> bool {
    dialoguer::Confirm::new()
        .with_prompt(format!("Submit to {repo}?"))
        .default(false)
        .interact()
        .unwrap_or(false)
}

/// True when the user is in interactive mode (i.e. we already prompted for
/// at least one of title/body) and they have custom filters that could be
/// useful to include. The actual prompt fires here on `true`.
fn should_prompt_for_filters(opts: &IssueArgs, info: &InfoOutput) -> bool {
    if opts.include_filters {
        return false; // already covered by the flag
    }
    let interactive = opts.title.is_none() || (opts.body.is_none() && opts.body_from.is_none());
    if !interactive {
        return false;
    }
    let custom = custom_filter_count(info);
    if custom == 0 {
        return false;
    }
    prompt_include_custom_filters(custom)
}

fn custom_filter_count(info: &InfoOutput) -> usize {
    info.filters.as_ref().map_or(0, |f| f.user + f.local)
}

fn prompt_include_custom_filters(count: usize) -> bool {
    dialoguer::Confirm::new()
        .with_prompt(format!(
            "Include {count} custom filter name(s) in the report? Helpful for debugging; may reveal project-internal command names."
        ))
        .default(false)
        .interact()
        .unwrap_or(false)
}

fn collect_env() -> EnvSnapshot {
    let shell = std::env::var("SHELL").ok().and_then(|p| {
        Path::new(&p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    });
    EnvSnapshot {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        shell,
        has_gh: which::which("gh").is_ok(),
        has_git: which::which("git").is_ok(),
    }
}

fn collect_filter_names(filters: &[tokf::config::ResolvedFilter]) -> FilterNames {
    let mut names = FilterNames {
        local: Vec::new(),
        user: Vec::new(),
        builtin: Vec::new(),
    };
    for f in filters {
        let name = f.relative_path.with_extension("").display().to_string();
        if f.priority == 0 {
            names.local.push(name);
        } else if f.priority == u8::MAX {
            names.builtin.push(name);
        } else {
            names.user.push(name);
        }
    }
    names.local.sort();
    names.user.sort();
    names.builtin.sort();
    names
}

/// Replace the user's home directory prefix in `s` with `~`. No-op when
/// `home` is `None` or empty. Idempotent.
fn redact_home(s: &str, home: Option<&Path>) -> String {
    let Some(h) = home else {
        return s.to_string();
    };
    let h_str = h.display().to_string();
    if h_str.is_empty() {
        return s.to_string();
    }
    s.replace(&h_str, "~")
}

struct MarkdownInputs<'a> {
    user_body: &'a str,
    info: &'a InfoOutput,
    env: &'a EnvSnapshot,
    filters: Option<&'a FilterNames>,
    home: Option<&'a Path>,
}

fn render_markdown(inputs: &MarkdownInputs<'_>) -> String {
    let mut out = String::new();
    render_summary(&mut out, inputs.user_body);
    render_environment(&mut out, inputs.info, inputs.env);
    render_installation(&mut out, inputs.info, inputs.home);
    if let Some(fl) = inputs.filters {
        render_filter_opt_in(&mut out, fl);
    }
    render_footer(&mut out, inputs.filters.is_some());
    out
}

fn render_summary(out: &mut String, user_body: &str) {
    out.push_str("## Summary\n\n");
    if user_body.trim().is_empty() {
        out.push_str("_(no description provided)_");
    } else {
        out.push_str(user_body);
    }
    out.push_str("\n\n");
}

fn render_environment(out: &mut String, info: &InfoOutput, env: &EnvSnapshot) {
    out.push_str("## Environment\n\n");
    let _ = writeln!(out, "- **tokf**: {}", info.version);
    let _ = writeln!(out, "- **OS / arch**: {} / {}", env.os, env.arch);
    if let Some(s) = &env.shell {
        let _ = writeln!(out, "- **Shell**: {s}");
    }
    let _ = writeln!(
        out,
        "- **Tools**: gh {}, git {}",
        if env.has_gh { "yes" } else { "no" },
        if env.has_git { "yes" } else { "no" },
    );
    out.push('\n');
}

fn render_installation(out: &mut String, info: &InfoOutput, home: Option<&Path>) {
    let r = |s: &str| redact_home(s, home);
    out.push_str("## tokf installation\n\n");
    let home_line = info
        .home_override
        .as_deref()
        .map_or_else(|| "(not set)".to_string(), &r);
    let _ = writeln!(out, "- TOKF_HOME: {home_line}");

    for dir in &info.search_dirs {
        if dir.scope == "built-in" {
            let _ = writeln!(out, "- [{}] {} (always available)", dir.scope, dir.path);
            continue;
        }
        let status = search_dir_status(dir.exists, dir.access);
        let _ = writeln!(out, "- [{}] {} ({status})", dir.scope, r(&dir.path));
    }

    let _ = writeln!(
        out,
        "- Tracking DB: {}",
        path_with_access(
            info.tracking_db.path.as_deref(),
            info.tracking_db.access,
            &r
        )
    );
    let _ = writeln!(
        out,
        "- Filter cache: {}",
        path_with_access(info.cache.path.as_deref(), info.cache.access, &r)
    );

    if let Some(f) = &info.filters {
        let _ = writeln!(
            out,
            "- Filters: {} built-in, {} user, {} local ({} total)",
            f.builtin, f.user, f.local, f.total
        );
    } else {
        out.push_str("- Filters: (discovery error)\n");
    }

    out.push_str("- Config files:\n");
    for entry in &info.config_files {
        let status = if entry.exists { "exists" } else { "not found" };
        let _ = writeln!(out, "  - [{}] {} — {status}", entry.scope, r(&entry.path));
    }
    out.push('\n');
}

const fn search_dir_status(exists: bool, access: Option<WriteAccess>) -> &'static str {
    if !exists {
        return "not found";
    }
    match access {
        Some(WriteAccess::Writable) => "exists, writable",
        Some(WriteAccess::ReadOnly) => "exists, read-only",
        _ => "exists",
    }
}

fn render_filter_opt_in(out: &mut String, fl: &FilterNames) {
    out.push_str("## Filters (opt-in)\n\n");
    render_filter_section(out, "built-in", &fl.builtin);
    render_filter_section(out, "user", &fl.user);
    render_filter_section(out, "local", &fl.local);
}

fn render_footer(out: &mut String, filters_included: bool) {
    out.push_str("<!--\n");
    out.push_str(
        "Excluded for privacy: hostname, username, machine UUID, auth tokens,\n\
         environment variables, command history, filter contents.\n",
    );
    if !filters_included {
        out.push_str("Filter names omitted (re-run with --include-filters to include them).\n");
    }
    out.push_str("-->\n\n");
    out.push_str("---\n_Generated by `tokf issue`. Review before posting._\n");
}

fn path_with_access(
    path: Option<&str>,
    access: Option<WriteAccess>,
    redact: &impl Fn(&str) -> String,
) -> String {
    path.map_or_else(
        || "(not available)".to_string(),
        |p| {
            let label = access.map_or("unknown", WriteAccess::label);
            format!("{} ({label})", redact(p))
        },
    )
}

fn render_filter_section(out: &mut String, label: &str, list: &[String]) {
    let _ = writeln!(out, "**{label}** ({})\n", list.len());
    if list.is_empty() {
        out.push_str("_(none)_\n\n");
        return;
    }
    for n in list {
        let _ = writeln!(out, "- `{n}`");
    }
    out.push('\n');
}

fn submit_via_gh(repo: &str, title: &str, body: &str) -> Result<String> {
    let mut tmp = tempfile::Builder::new()
        .prefix("tokf-issue-")
        .suffix(".md")
        .tempfile()
        .context("creating temp file for body")?;
    tmp.write_all(body.as_bytes())
        .context("writing body to temp file")?;
    tmp.flush().ok();
    let output = std::process::Command::new("gh")
        .args([
            "issue",
            "create",
            "--repo",
            repo,
            "--title",
            title,
            "--body-file",
        ])
        .arg(tmp.path())
        .output()
        .context("invoking gh")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh exited with {} — {}", output.status, stderr.trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().last().unwrap_or("").trim().to_string())
}

fn fallback_print(repo: &str, title: &str, body: &str) {
    println!("{body}");
    match build_issues_new_url(repo, title, body) {
        Some(url) => {
            eprintln!("\n[tokf] Pre-filled URL (open in your browser):");
            eprintln!("{url}");
        }
        None => {
            eprintln!(
                "\n[tokf] Body too long for URL pre-fill. Open https://github.com/{repo}/issues/new and paste the markdown above."
            );
        }
    }
}

fn build_issues_new_url(repo: &str, title: &str, body: &str) -> Option<String> {
    let url = format!(
        "https://github.com/{}/issues/new?title={}&body={}",
        repo,
        url_encode(title),
        url_encode(body),
    );
    if url.len() > ISSUES_NEW_URL_BUDGET {
        None
    } else {
        Some(url)
    }
}

/// Percent-encode using the unreserved set from RFC 3986.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests;
