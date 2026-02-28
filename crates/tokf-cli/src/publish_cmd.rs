use std::io::BufRead as _;
use std::path::Path;

use tokf::auth::credentials;
use tokf::config;
use tokf::remote::publish_client;

/// Entry point for the `tokf publish` subcommand.
pub fn cmd_publish(filter_name: &str, dry_run: bool, update_tests: bool) -> i32 {
    let result = if update_tests {
        publish_update_tests(filter_name, dry_run)
    } else {
        publish(filter_name, dry_run)
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// Resolve a local filter by name, rejecting stdlib filters.
fn resolve_local_filter(filter_name: &str) -> anyhow::Result<config::ResolvedFilter> {
    let search_dirs = config::default_search_dirs();
    let resolved = config::discover_all_filters(&search_dirs)?;
    let resolved_filter = resolved
        .into_iter()
        .find(|f| f.matches_name(filter_name))
        .ok_or_else(|| anyhow::anyhow!("filter not found: {filter_name}"))?;

    if resolved_filter.priority == tokf::config::STDLIB_PRIORITY {
        anyhow::bail!(
            "'{filter_name}' is a built-in stdlib filter — \
             eject it first with `tokf eject {filter_name}`"
        );
    }

    Ok(resolved_filter)
}

/// Load credentials and build an authenticated HTTP client.
fn authed_client() -> anyhow::Result<(reqwest::blocking::Client, credentials::LoadedAuth)> {
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
    Ok((client, auth))
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

/// If `lua_script.file` is set, read the external file and embed its content
/// as `lua_script.source` so the filter TOML is self-contained for publishing.
///
/// Returns the (possibly modified) filter bytes. When no `lua_script.file` is
/// present, the original bytes are returned unchanged.
///
/// The script path is canonicalized and must reside within (or under) the
/// filter file's parent directory to prevent path-traversal attacks.
fn inline_lua_script(filter_bytes: Vec<u8>, filter_path: &Path) -> anyhow::Result<Vec<u8>> {
    let toml_str = std::str::from_utf8(&filter_bytes)
        .map_err(|_| anyhow::anyhow!("filter TOML is not valid UTF-8"))?;
    let mut cfg: tokf_common::config::types::FilterConfig =
        toml::from_str(toml_str).map_err(|e| anyhow::anyhow!("invalid filter TOML: {e}"))?;

    let script_file = match cfg.lua_script.as_ref().and_then(|s| s.file.as_ref()) {
        Some(f) => f.clone(),
        None => return Ok(filter_bytes),
    };

    let base_dir = filter_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot resolve filter directory: {e}"))?;
    let script_path = base_dir.join(&script_file);
    let canonical_script = script_path.canonicalize().map_err(|e| {
        anyhow::anyhow!(
            "cannot resolve lua_script.file '{}': {e}",
            script_path.display()
        )
    })?;

    if !canonical_script.starts_with(&base_dir) {
        anyhow::bail!(
            "lua_script.file '{}' escapes the filter directory — \
             the script must reside within '{}'",
            script_file,
            base_dir.display()
        );
    }

    let source = std::fs::read_to_string(&canonical_script).map_err(|e| {
        anyhow::anyhow!(
            "cannot read lua_script.file '{}': {e}",
            canonical_script.display()
        )
    })?;

    if let Some(script_cfg) = cfg.lua_script.as_mut() {
        script_cfg.source = Some(source);
        script_cfg.file = None;
    }

    eprintln!("[tokf] inlined lua_script.file '{script_file}' into filter source");
    let serialized =
        toml::to_string_pretty(&cfg).map_err(|e| anyhow::anyhow!("TOML serialize error: {e}"))?;
    Ok(serialized.into_bytes())
}

// ── Publish flow ────────────────────────────────────────────────────────────

fn publish(filter_name: &str, dry_run: bool) -> anyhow::Result<i32> {
    let filter_name = filter_name.strip_suffix(".toml").unwrap_or(filter_name);
    let resolved_filter = resolve_local_filter(filter_name)?;

    let filter_bytes = std::fs::read(&resolved_filter.source_path)?;
    let filter_bytes = inline_lua_script(filter_bytes, &resolved_filter.source_path)?;
    let (content_hash, command_pattern) = hash_filter(&filter_bytes)?;
    let test_files = collect_test_files(&resolved_filter)?;

    eprintln!("[tokf] publishing filter: {filter_name}");
    eprintln!("  Command: {command_pattern}");
    eprintln!("  Hash:    {content_hash}");
    eprintln!("  Tests:   {} file(s)", test_files.len());

    if dry_run {
        eprintln!("[tokf] dry-run: no files uploaded");
        return Ok(0);
    }

    ensure_license_accepted()?;
    let (client, auth) = authed_client()?;

    let (is_new, resp) = publish_client::publish_filter(
        &client,
        &auth.server_url,
        &auth.token,
        &filter_bytes,
        &test_files,
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

// ── Update-tests flow ───────────────────────────────────────────────────────

fn publish_update_tests(filter_name: &str, dry_run: bool) -> anyhow::Result<i32> {
    let filter_name = filter_name.strip_suffix(".toml").unwrap_or(filter_name);
    let resolved_filter = resolve_local_filter(filter_name)?;

    let filter_bytes = std::fs::read(&resolved_filter.source_path)?;
    let (content_hash, _) = hash_filter(&filter_bytes)?;
    let test_files = collect_test_files(&resolved_filter)?;

    if test_files.is_empty() {
        anyhow::bail!("no test files found for {filter_name}");
    }

    // Validate test files locally before uploading
    for (filename, bytes) in &test_files {
        tokf_common::test_case::validate(bytes).map_err(|e| anyhow::anyhow!("{filename}: {e}"))?;
    }

    eprintln!("[tokf] updating test suite for: {filter_name}");
    eprintln!("  Hash:  {content_hash}");
    eprintln!("  Tests: {} file(s)", test_files.len());

    if dry_run {
        for (name, _) in &test_files {
            eprintln!("  - {name}");
        }
        eprintln!("[tokf] dry-run: no files uploaded");
        return Ok(0);
    }

    let (client, auth) = authed_client()?;

    let resp = publish_client::update_tests(
        &client,
        &auth.server_url,
        &auth.token,
        &content_hash,
        &test_files,
    )?;

    eprintln!(
        "[tokf] updated test suite for {filter_name} ({} file(s))",
        resp.test_count
    );
    eprintln!("URL: {}", resp.registry_url);
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
        let cfg: tokf_common::config::types::FilterConfig =
            toml::from_str(r#"command = "my-cmd""#).unwrap();
        let hash = tokf_common::hash::canonical_hash(&cfg).unwrap_or_default();
        let resolved = config::ResolvedFilter {
            config: cfg,
            hash,
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
    fn inline_lua_script_embeds_file_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        let script_path = dir.path().join("transform.luau");

        std::fs::write(&script_path, "return output:upper()").unwrap();
        std::fs::write(
            &filter_path,
            r#"command = "my-cmd"

[lua_script]
lang = "luau"
file = "transform.luau"
"#,
        )
        .unwrap();

        let filter_bytes = std::fs::read(&filter_path).unwrap();
        let result = inline_lua_script(filter_bytes, &filter_path).unwrap();

        let cfg: tokf_common::config::types::FilterConfig =
            toml::from_str(std::str::from_utf8(&result).unwrap()).unwrap();
        let script = cfg.lua_script.unwrap();
        assert!(script.file.is_none(), "file should be removed");
        assert_eq!(script.source.unwrap(), "return output:upper()");
    }

    #[test]
    fn inline_lua_script_rejects_path_traversal() {
        let dir = tempfile::TempDir::new().unwrap();
        let subdir = dir.path().join("filters");
        std::fs::create_dir_all(&subdir).unwrap();

        // Create a file outside the filter directory
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, "sensitive data").unwrap();

        let filter_path = subdir.join("my-filter.toml");
        std::fs::write(
            &filter_path,
            r#"command = "my-cmd"

[lua_script]
lang = "luau"
file = "../secret.txt"
"#,
        )
        .unwrap();

        let filter_bytes = std::fs::read(&filter_path).unwrap();
        let result = inline_lua_script(filter_bytes, &filter_path);
        assert!(result.is_err(), "path traversal should be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("escapes the filter directory"),
            "expected traversal error, got: {msg}"
        );
    }

    #[test]
    fn inline_lua_script_allows_subdirectory() {
        let dir = tempfile::TempDir::new().unwrap();
        let scripts_dir = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts_dir).unwrap();

        std::fs::write(scripts_dir.join("helper.luau"), "return 'ok'").unwrap();

        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(
            &filter_path,
            r#"command = "my-cmd"

[lua_script]
lang = "luau"
file = "scripts/helper.luau"
"#,
        )
        .unwrap();

        let filter_bytes = std::fs::read(&filter_path).unwrap();
        let result = inline_lua_script(filter_bytes, &filter_path).unwrap();

        let cfg: tokf_common::config::types::FilterConfig =
            toml::from_str(std::str::from_utf8(&result).unwrap()).unwrap();
        let script = cfg.lua_script.unwrap();
        assert!(script.file.is_none());
        assert_eq!(script.source.unwrap(), "return 'ok'");
    }

    #[test]
    fn inline_lua_script_empty_file_produces_empty_source() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(dir.path().join("empty.luau"), "").unwrap();
        std::fs::write(
            &filter_path,
            r#"command = "my-cmd"

[lua_script]
lang = "luau"
file = "empty.luau"
"#,
        )
        .unwrap();

        let filter_bytes = std::fs::read(&filter_path).unwrap();
        let result = inline_lua_script(filter_bytes, &filter_path).unwrap();

        let cfg: tokf_common::config::types::FilterConfig =
            toml::from_str(std::str::from_utf8(&result).unwrap()).unwrap();
        let script = cfg.lua_script.unwrap();
        assert_eq!(script.source.unwrap(), "");
    }

    #[test]
    fn inline_lua_script_hash_stable_when_no_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        let toml_str = r#"command = "my-cmd""#;
        std::fs::write(&filter_path, toml_str).unwrap();

        let filter_bytes = toml_str.as_bytes().to_vec();
        let (hash_before, _) = hash_filter(&filter_bytes).unwrap();
        let result = inline_lua_script(filter_bytes, &filter_path).unwrap();
        let (hash_after, _) = hash_filter(&result).unwrap();
        assert_eq!(
            hash_before, hash_after,
            "hash should be stable when no inlining occurs"
        );
    }

    #[test]
    fn inline_lua_script_noop_without_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        let toml_str = r#"command = "my-cmd""#;
        std::fs::write(&filter_path, toml_str).unwrap();

        let filter_bytes = toml_str.as_bytes().to_vec();
        let result = inline_lua_script(filter_bytes.clone(), &filter_path).unwrap();
        assert_eq!(result, filter_bytes, "should return unchanged bytes");
    }

    #[test]
    fn collect_test_files_returns_empty_when_no_test_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        let cfg: tokf_common::config::types::FilterConfig =
            toml::from_str(r#"command = "my-cmd""#).unwrap();
        let hash = tokf_common::hash::canonical_hash(&cfg).unwrap_or_default();
        let resolved = config::ResolvedFilter {
            config: cfg,
            hash,
            source_path: filter_path,
            relative_path: std::path::PathBuf::from("my-filter.toml"),
            priority: 0,
        };

        let files = collect_test_files(&resolved).unwrap();
        assert!(files.is_empty());
    }
}
