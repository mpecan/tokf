use std::io::BufRead as _;
use std::path::Path;

use tokf::auth::credentials;
use tokf::config;
use tokf::remote::publish_client;

/// Entry point for the `tokf publish` subcommand.
pub fn cmd_publish(filter_name: &str, dry_run: bool) -> i32 {
    match publish(filter_name, dry_run) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn publish(filter_name: &str, dry_run: bool) -> anyhow::Result<i32> {
    let filter_name = filter_name.strip_suffix(".toml").unwrap_or(filter_name);

    let search_dirs = config::default_search_dirs();
    let resolved = config::discover_all_filters(&search_dirs)?;
    let resolved_filter = resolved
        .iter()
        .find(|f| f.matches_name(filter_name))
        .ok_or_else(|| anyhow::anyhow!("filter not found: {filter_name}"))?;

    if resolved_filter.priority == tokf::config::STDLIB_PRIORITY {
        anyhow::bail!(
            "'{filter_name}' is a built-in stdlib filter — \
             eject it first with `tokf eject {filter_name}`"
        );
    }

    let filter_bytes = std::fs::read(&resolved_filter.source_path)?;
    let (content_hash, command_pattern) = hash_filter(&filter_bytes)?;
    let test_files = collect_test_files(resolved_filter)?;

    eprintln!("[tokf] publishing filter: {filter_name}");
    eprintln!("  Command: {command_pattern}");
    eprintln!("  Hash:    {content_hash}");
    eprintln!("  Tests:   {} file(s)", test_files.len());

    if dry_run {
        eprintln!("[tokf] dry-run: no files uploaded");
        return Ok(0);
    }

    upload_to_registry(filter_name, filter_bytes, test_files)
}

/// Parse filter bytes and return `(content_hash, command_pattern)`.
fn hash_filter(filter_bytes: &[u8]) -> anyhow::Result<(String, String)> {
    let toml_str = std::str::from_utf8(filter_bytes)
        .map_err(|_| anyhow::anyhow!("filter TOML is not valid UTF-8"))?;
    let cfg: tokf_common::config::types::FilterConfig =
        toml::from_str(toml_str).map_err(|e| anyhow::anyhow!("invalid filter TOML: {e}"))?;
    let hash =
        tokf_common::hash::canonical_hash(&cfg).map_err(|e| anyhow::anyhow!("hash error: {e}"))?;
    Ok((hash, cfg.command.first().to_string()))
}

/// Prompt for license, load auth, and upload to the registry.
fn upload_to_registry(
    filter_name: &str,
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
) -> anyhow::Result<i32> {
    ensure_license_accepted()?;

    let auth = credentials::load()
        .ok_or_else(|| anyhow::anyhow!("not logged in — run `tokf auth login` first"))?;
    if auth.is_expired() {
        anyhow::bail!("token has expired — run `tokf auth login` to re-authenticate");
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| anyhow::anyhow!("could not build HTTP client: {e}"))?;

    let (is_new, resp) = publish_client::publish_filter(
        &client,
        &auth.server_url,
        &auth.token,
        filter_bytes,
        test_files,
    )?;

    if is_new {
        eprintln!("[tokf] published {filter_name}  (201 Created)");
    } else {
        eprintln!("[tokf] already exists  (200 OK)");
    }
    eprintln!("Hash:    {}", resp.content_hash);
    eprintln!("Author:  {}", resp.author);
    eprintln!("URL:     {}", resp.registry_url);
    Ok(0)
}

fn collect_test_files(
    resolved_filter: &config::ResolvedFilter,
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let stem = resolved_filter
        .relative_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let test_dir_name = format!("{stem}_test");
    let source_test_dir = resolved_filter
        .source_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&test_dir_name);

    if !source_test_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&source_test_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let filename = entry.file_name().to_string_lossy().to_string();
            let bytes = std::fs::read(&path)?;
            files.push((filename, bytes));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

fn ensure_license_accepted() -> anyhow::Result<()> {
    let accepted = credentials::load()
        .and_then(|a| a.mit_license_accepted)
        .unwrap_or(false);

    if accepted {
        return Ok(());
    }

    eprintln!("[tokf] This filter will be published under the MIT license.");
    eprintln!("[tokf] Anyone may use, modify, and distribute it with attribution.");
    eprint!("[tokf] Accept MIT license? [y/N]: ");

    let mut input = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut input)
        .map_err(|e| anyhow::anyhow!("could not read input: {e}"))?;

    if input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("yes") {
        credentials::save_license_accepted(true)?;
        eprintln!("[tokf] MIT license accepted.");
        Ok(())
    } else {
        anyhow::bail!("MIT license not accepted — publish cancelled")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn resolve_fails_for_unknown_filter() {
        let search_dirs = config::default_search_dirs();
        let resolved = config::discover_all_filters(&search_dirs).unwrap();
        let found = resolved
            .iter()
            .find(|f| f.matches_name("nonexistent/xyz-abc-filter-99"));
        assert!(found.is_none(), "expected no match for nonexistent filter");
    }

    #[test]
    fn collect_test_files_from_adjacent_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_dir = dir.path().join("myns");
        std::fs::create_dir_all(&filter_dir).unwrap();

        // Create filter
        let filter_path = filter_dir.join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        // Create adjacent _test/ dir with test files
        let test_dir = filter_dir.join("my-filter_test");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::fs::write(test_dir.join("basic.toml"), b"name = \"basic\"").unwrap();
        std::fs::write(test_dir.join("edge.toml"), b"name = \"edge\"").unwrap();

        // Build a minimal ResolvedFilter pointing to our temp file
        let resolved = config::ResolvedFilter {
            config: toml::from_str(r#"command = "my-cmd""#).unwrap(),
            source_path: filter_path,
            relative_path: std::path::PathBuf::from("myns/my-filter.toml"),
            priority: 0,
        };

        let files = collect_test_files(&resolved).unwrap();
        assert_eq!(files.len(), 2, "expected 2 test files");
        let names: std::collections::HashSet<_> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains("basic.toml"));
        assert!(names.contains("edge.toml"));
    }

    #[test]
    fn collect_test_files_returns_empty_when_no_test_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        let resolved = config::ResolvedFilter {
            config: toml::from_str(r#"command = "my-cmd""#).unwrap(),
            source_path: filter_path,
            relative_path: std::path::PathBuf::from("my-filter.toml"),
            priority: 0,
        };

        let files = collect_test_files(&resolved).unwrap();
        assert!(files.is_empty());
    }
}
