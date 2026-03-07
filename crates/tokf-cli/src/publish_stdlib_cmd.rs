use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use tokf::publish_shared::collect_test_files_resolved;
use tokf::remote::http::Client;

/// Entry point for the `tokf publish-stdlib` subcommand.
pub fn cmd_publish_stdlib(registry_url: &str, token: &str, dry_run: bool) -> i32 {
    match publish_stdlib(registry_url, token, dry_run) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

// ── Request/response types (mirrors server types) ───────────────────────────

#[derive(Debug, Serialize)]
struct StdlibPublishRequest {
    filters: Vec<StdlibFilterEntry>,
    version: String,
}

#[derive(Debug, Serialize)]
struct StdlibFilterEntry {
    filter_toml: String,
    test_files: Vec<StdlibTestFile>,
    author_github_username: String,
}

#[derive(Debug, Serialize)]
struct StdlibTestFile {
    filename: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct StdlibPublishResponse {
    published: usize,
    skipped: usize,
    failed: Vec<StdlibFailure>,
}

#[derive(Debug, Deserialize)]
struct StdlibFailure {
    command_pattern: String,
    error: String,
}

// ── Core logic ──────────────────────────────────────────────────────────────

fn publish_stdlib(registry_url: &str, token: &str, dry_run: bool) -> anyhow::Result<i32> {
    let filters_dir = Path::new("crates/tokf-cli/filters");
    if !filters_dir.is_dir() {
        anyhow::bail!(
            "filters directory not found: {}\nRun this command from the repository root.",
            filters_dir.display()
        );
    }

    let entries = collect_stdlib_entries(filters_dir)?;
    eprintln!("[publish-stdlib] Collected {} filters", entries.len());

    if entries.is_empty() {
        eprintln!("[publish-stdlib] No filters found.");
        return Ok(0);
    }

    let version = env!("CARGO_PKG_VERSION").to_string();
    eprintln!("[publish-stdlib] Version: {version}");

    if dry_run {
        let payload = serde_json::to_string_pretty(&StdlibPublishRequest {
            filters: entries,
            version,
        })?;
        println!("{payload}");
        eprintln!("[publish-stdlib] Dry run — payload printed above.");
        return Ok(0);
    }

    let client = Client::new(registry_url, Some(token))?;
    let tally = publish_entries_one_by_one(&client, entries, &version);

    eprintln!();
    eprintln!("[publish-stdlib] Published: {}", tally.published);
    eprintln!("[publish-stdlib] Skipped:   {}", tally.skipped);
    if !tally.failures.is_empty() {
        eprintln!("[publish-stdlib] Failed:    {}", tally.failures.len());
        for (cmd, err) in &tally.failures {
            eprintln!("  {cmd} — {err}");
        }
        return Ok(1);
    }

    Ok(0)
}

struct PublishTally {
    published: usize,
    skipped: usize,
    failures: Vec<(String, String)>,
}

/// Publish each filter individually so a single failure doesn't mask others.
fn publish_entries_one_by_one(
    client: &Client,
    entries: Vec<StdlibFilterEntry>,
    version: &str,
) -> PublishTally {
    let total = entries.len();
    let mut published = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<(String, String)> = Vec::new();

    for (i, entry) in entries.into_iter().enumerate() {
        let label = entry
            .filter_toml
            .lines()
            .find(|l| l.trim_start().starts_with("command"))
            .unwrap_or("<unknown>")
            .to_string();

        eprint!("[publish-stdlib] [{}/{}] {label} ... ", i + 1, total);

        let req = StdlibPublishRequest {
            filters: vec![entry],
            version: version.to_string(),
        };
        match client.post::<_, StdlibPublishResponse>("/api/filters/publish-stdlib", &req) {
            Ok(resp) => {
                published += resp.published;
                skipped += resp.skipped;
                if resp.failed.is_empty() {
                    if resp.published > 0 {
                        eprintln!("published");
                    } else {
                        eprintln!("skipped (already exists)");
                    }
                } else {
                    for f in &resp.failed {
                        eprintln!("FAILED: {}", f.error);
                        failures.push((f.command_pattern.clone(), f.error.clone()));
                    }
                }
            }
            Err(e) => {
                eprintln!("ERROR: {e:#}");
                failures.push((label, format!("{e:#}")));
            }
        }
    }

    PublishTally {
        published,
        skipped,
        failures,
    }
}

/// Walk the filters directory, collecting each filter TOML with its resolved
/// test files (fixtures inlined).
fn collect_stdlib_entries(filters_dir: &Path) -> anyhow::Result<Vec<StdlibFilterEntry>> {
    let mut filter_paths = Vec::new();
    collect_filter_files(filters_dir, &mut filter_paths)?;
    filter_paths.sort();

    let fallback_author = resolve_fallback_author();
    let mut entries = Vec::with_capacity(filter_paths.len());

    for path in &filter_paths {
        let filter_toml = std::fs::read_to_string(path)?;
        let author = resolve_author(path).unwrap_or_else(|| fallback_author.clone());

        let raw_tests = collect_test_files_resolved(path)?;
        let test_files: Vec<StdlibTestFile> = raw_tests
            .into_iter()
            .map(|(filename, bytes)| StdlibTestFile {
                filename,
                content: String::from_utf8_lossy(&bytes).to_string(),
            })
            .collect();

        eprintln!(
            "[publish-stdlib]   {} (author: {}, tests: {})",
            path.display(),
            author,
            count_test_files(&test_files),
        );

        entries.push(StdlibFilterEntry {
            filter_toml,
            test_files,
            author_github_username: author,
        });
    }

    Ok(entries)
}

fn count_test_files(files: &[StdlibTestFile]) -> usize {
    files.len()
}

/// Recursively find all *.toml files that aren't inside _test/ directories.
fn collect_filter_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with("_test") {
                collect_filter_files(&path, out)?;
            }
        } else if path.extension().is_some_and(|e| e == "toml") {
            out.push(path);
        }
    }
    Ok(())
}

/// Best-effort: resolve the GitHub username of the last committer for a file.
fn resolve_author(path: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ae", "--"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let email = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if email.is_empty() {
        return None;
    }
    // Extract username from GitHub noreply: 1234+user@users.noreply.github.com
    if let Some(rest) = email.strip_suffix("@users.noreply.github.com") {
        return Some(rest.rsplit('+').next().unwrap_or(rest).to_string());
    }
    // Non-noreply emails can't be reliably mapped to GitHub usernames
    None
}

/// Determine the fallback author (repo owner or "mpecan").
fn resolve_fallback_author() -> String {
    let output = std::process::Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "-q",
            ".nameWithOwner",
        ])
        .output()
        .ok();
    if let Some(out) = output
        && out.status.success()
    {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(owner) = name.split('/').next()
            && !owner.is_empty()
        {
            return owner.to_string();
        }
    }
    "mpecan".to_string()
}
