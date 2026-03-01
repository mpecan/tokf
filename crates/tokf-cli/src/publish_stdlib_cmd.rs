use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

    if dry_run {
        let payload = serde_json::to_string_pretty(&StdlibPublishRequest { filters: entries })?;
        println!("{payload}");
        eprintln!("[publish-stdlib] Dry run — payload printed above.");
        return Ok(0);
    }

    let client = Client::new(registry_url, Some(token))?;
    let tally = publish_entries_one_by_one(&client, entries);

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
fn publish_entries_one_by_one(client: &Client, entries: Vec<StdlibFilterEntry>) -> PublishTally {
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
        let test_files = collect_and_resolve_tests(path)?;

        eprintln!(
            "[publish-stdlib]   {} (author: {}, tests: {})",
            path.display(),
            author,
            test_files.len()
        );

        entries.push(StdlibFilterEntry {
            filter_toml,
            test_files,
            author_github_username: author,
        });
    }

    Ok(entries)
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

/// Collect test TOML files from the adjacent `{stem}_test/` directory,
/// resolving `fixture = "file.txt"` references to `inline` content.
fn collect_and_resolve_tests(filter_path: &Path) -> anyhow::Result<Vec<StdlibTestFile>> {
    let stem = filter_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let test_dir = filter_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}_test"));

    if !test_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&test_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }

        let content = std::fs::read_to_string(&path)?;
        let resolved = resolve_fixtures_in_test(&content, &test_dir)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

        files.push(StdlibTestFile {
            filename,
            content: resolved,
        });
    }
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(files)
}

/// Replace `fixture = "file.txt"` with `inline = "<file contents>"` in a test
/// TOML string. Fixture files are resolved relative to `test_dir`.
fn resolve_fixtures_in_test(content: &str, test_dir: &Path) -> anyhow::Result<String> {
    // Parse to check if there's a fixture reference
    let case: tokf_common::test_case::TestCase =
        toml::from_str(content).map_err(|e| anyhow::anyhow!("invalid test TOML: {e}"))?;

    let Some(fixture_name) = case.fixture else {
        // No fixture — return as-is
        return Ok(content.to_string());
    };

    if case.inline.is_some() {
        // Has both — inline takes priority, return as-is
        return Ok(content.to_string());
    }

    let fixture_path = test_dir.join(&fixture_name);
    let fixture_content = std::fs::read_to_string(&fixture_path)
        .map_err(|e| anyhow::anyhow!("cannot read fixture '{fixture_name}': {e}"))?;
    let fixture_content = fixture_content.trim_end();

    // Rebuild the TOML with inline instead of fixture
    let mut result = String::new();
    let mut skipped_fixture = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("fixture") && trimmed.contains('=') {
            skipped_fixture = true;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if !skipped_fixture {
        anyhow::bail!("expected fixture line but did not find it");
    }

    // Insert inline using TOML multi-line literal string (triple single-quotes).
    // Literal strings don't interpret backslashes, which is critical for fixture
    // content that may contain paths, regex patterns, or separator lines like \---.
    let insert_pos = result.find("[[expect]]").unwrap_or(result.len());
    let inline_value = format!("inline = '''\n{fixture_content}\n'''\n");
    result.insert_str(insert_pos, &inline_value);

    Ok(result)
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn resolve_fixtures_replaces_fixture_with_inline() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("output.txt"), "hello world\n").unwrap();

        let toml = r#"name = "test"
fixture = "output.txt"
exit_code = 0

[[expect]]
contains = "hello"
"#;
        let resolved = resolve_fixtures_in_test(toml, dir.path()).unwrap();
        assert!(!resolved.contains("fixture"), "fixture should be removed");
        assert!(resolved.contains("inline"), "should contain inline");
        assert!(
            resolved.contains("hello world"),
            "should contain fixture content"
        );
    }

    #[test]
    fn resolve_fixtures_preserves_inline() {
        let toml = r#"name = "test"
inline = "already inline"

[[expect]]
equals = "already inline"
"#;
        let resolved = resolve_fixtures_in_test(toml, Path::new("/nonexistent")).unwrap();
        assert_eq!(resolved, toml, "should be unchanged");
    }

    #[test]
    fn resolve_fixtures_uses_literal_strings_for_backslashes() {
        let dir = tempfile::TempDir::new().unwrap();
        // Fixture content with backslashes (like gradle's \--- separators)
        std::fs::write(dir.path().join("output.txt"), "\\--- foo\n\\--- bar\n").unwrap();

        let toml = r#"name = "test"
fixture = "output.txt"
exit_code = 0

[[expect]]
contains = "foo"
"#;
        let resolved = resolve_fixtures_in_test(toml, dir.path()).unwrap();
        // Must use ''' (literal string) so backslashes aren't interpreted
        assert!(
            resolved.contains("'''"),
            "should use literal string quotes, got:\n{resolved}"
        );
        assert!(
            !resolved.contains(r#"""""#),
            "should NOT use basic string quotes"
        );
        assert!(
            resolved.contains("\\--- foo"),
            "backslashes should be preserved literally"
        );
    }

    #[test]
    fn resolve_fixtures_errors_on_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let toml = r#"name = "test"
fixture = "missing.txt"

[[expect]]
contains = "x"
"#;
        let result = resolve_fixtures_in_test(toml, dir.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("missing.txt"),
            "error should mention the file"
        );
    }
}
