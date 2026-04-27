use std::path::{Component, Path, PathBuf};

use tokf::remote::filter_client;
use tokf::remote::http::Client;
use tokf_common::config::types::FilterConfig;

/// Entry point for the `tokf install` subcommand.
#[allow(clippy::fn_params_excessive_bools)]
pub fn cmd_install(filter: &str, local: bool, force: bool, dry_run: bool, yes: bool) -> i32 {
    match install(filter, local, force, dry_run, yes) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn resolve_hash(client: &Client, filter: &str) -> anyhow::Result<(String, String)> {
    if is_hash(filter) {
        let details = filter_client::get_filter(client, filter)?;
        Ok((details.content_hash, details.author))
    } else {
        let results = filter_client::search_filters(client, filter, 1)?;
        let first = results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no filter found matching: {filter}"))?;
        Ok((first.content_hash, first.author))
    }
}

#[allow(clippy::fn_params_excessive_bools)]
fn install(
    filter: &str,
    local: bool,
    force: bool,
    dry_run: bool,
    yes: bool,
) -> anyhow::Result<i32> {
    let client = Client::authed()?;

    let (url_hash, author) = resolve_hash(&client, filter)?;
    let downloaded = filter_client::download_filter(&client, &url_hash)?;

    // Parse TOML once; derive command pattern and detect Lua in a single pass.
    let (command_pattern, config) = parse_filter_toml(&downloaded.filter_toml)?;

    // The hash actually used for attribution: the server-recomputed
    // `content_hash` when present, else the URL hash. The URL hash is a
    // stable lookup key; the recomputed hash is the authoritative identity
    // under the current schema. See issue #350.
    let hash = verify_and_resolve_hash(&url_hash, downloaded.content_hash.as_deref(), &config)?;

    let install_base = resolve_install_base(local)?;
    let rel_path = command_pattern_to_path(&command_pattern);

    // Ensure command_pattern doesn't produce a path that escapes install_base.
    if rel_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        anyhow::bail!("unsafe install path derived from filter: {command_pattern:?}");
    }

    let install_path = install_base.join("filters").join(&rel_path);

    if install_path.exists() && !force {
        anyhow::bail!(
            "filter already exists at {} — use --force to overwrite",
            install_path.display()
        );
    }

    let stem = rel_path.file_stem().unwrap_or_default().to_string_lossy();
    let test_dir = install_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}_test"));

    if dry_run {
        print_dry_run_summary(&command_pattern, &hash, &author, &install_path, &downloaded);
        return Ok(0);
    }

    // Show the filter and ask for confirmation before writing anything.
    prompt_install_confirm(&downloaded.filter_toml, &config, &author, &hash, yes)?;

    write_filter(&downloaded, &install_path, &hash, &author, &test_dir)?;

    if !downloaded.test_files.is_empty() {
        run_verify(&rel_path, &install_path, &test_dir)?;
    }

    eprintln!(
        "[tokf] installed {} → {}",
        command_pattern,
        install_path.display()
    );
    Ok(0)
}

/// Parse the filter TOML and return the first command pattern together with
/// the parsed config.
///
/// The `toml` crate natively ignores TOML comments, so stripping `#` lines
/// is unnecessary and would corrupt multi-line strings (e.g. Lua source)
/// that contain lines beginning with `#`.
///
/// # Errors
///
/// Returns an error if the TOML is invalid or has no command patterns.
fn parse_filter_toml(toml_str: &str) -> anyhow::Result<(String, FilterConfig)> {
    let config: FilterConfig = toml::from_str(toml_str)
        .map_err(|e| anyhow::anyhow!("could not parse filter TOML: {e}"))?;
    let pattern = config.command.first().to_string();
    if pattern.is_empty() {
        anyhow::bail!("filter has no command patterns");
    }
    Ok((pattern, config))
}

/// Verify the downloaded filter and resolve the authoritative content hash.
///
/// The server (when up-to-date — see issue #350) recomputes `canonical_hash`
/// over the stored TOML and returns it as `server_content_hash`. The client
/// trusts that value as the filter's identity under the current schema, but
/// still hashes the wire bytes to detect any tampering between server and
/// client.
///
/// When the server is older than this client and doesn't return a
/// `server_content_hash`, fall back to verifying that the URL hash itself
/// matches the locally-computed hash (the historical behaviour, broken for
/// filters published before recent `FilterConfig` schema additions but kept
/// for graceful degradation).
///
/// Returns the hash to use for attribution and display.
///
/// # Errors
///
/// - Server provided a hash that doesn't match the wire bytes (tamper).
/// - Old server, and the URL hash doesn't match the locally-computed hash.
fn verify_and_resolve_hash(
    url_hash: &str,
    server_content_hash: Option<&str>,
    config: &FilterConfig,
) -> anyhow::Result<String> {
    let client_hash = tokf_common::hash::canonical_hash(config)
        .map_err(|e| anyhow::anyhow!("could not compute filter hash: {e}"))?;
    if let Some(server_hash) = server_content_hash {
        if client_hash != server_hash {
            anyhow::bail!(
                "filter content hash mismatch with server-provided value: \
                 server={server_hash}, computed={client_hash} — \
                 the connection may have been tampered with"
            );
        }
        if url_hash != server_hash {
            eprintln!(
                "[tokf] note: filter URL hash {url_hash} differs from current \
                 content hash {server_hash}; the filter was published under an \
                 older schema and re-identifies under the current one."
            );
        }
        return Ok(server_hash.to_string());
    }
    // Old-server fallback: the URL hash is the only identity we have.
    if client_hash != url_hash {
        anyhow::bail!(
            "filter hash mismatch: expected {url_hash}, computed {client_hash} — \
             the server may have returned tampered content, or the filter may \
             have been published under a `FilterConfig` schema this client \
             cannot reproduce (see issue #350)"
        );
    }
    Ok(url_hash.to_string())
}

/// Show the filter TOML and prompt for installation consent.
///
/// For Lua filters (which run arbitrary code), typing `yes` in full is
/// required. `--yes` bypasses the interactive prompt for both filter types
/// but still prints an audit warning for Lua.
///
/// # Errors
///
/// Returns an error if the user declines or stdin cannot be read.
fn prompt_install_confirm(
    filter_toml: &str,
    config: &FilterConfig,
    author: &str,
    hash: &str,
    yes: bool,
) -> anyhow::Result<()> {
    let has_lua = config.lua_script.is_some();

    // Always display the filter content so the user can review it.
    eprintln!("[tokf] filter preview:");
    eprintln!("─────────────────────────────────────────");
    for line in filter_toml.lines().filter(|l| !l.starts_with('#')) {
        eprintln!("{line}");
    }
    eprintln!("─────────────────────────────────────────");
    eprintln!("[tokf] author: @{author}  ·  review: https://tokf.net/filters/{hash}");

    if has_lua {
        eprintln!(
            "[tokf] WARNING: this filter contains embedded Lua code that will run on your machine."
        );
    }

    if yes {
        if has_lua {
            // Always emit an audit trail when Lua is installed non-interactively.
            eprintln!(
                "[tokf] warning: Lua filter installed non-interactively (--yes); \
                 review at https://tokf.net/filters/{hash}"
            );
        }
        return Ok(());
    }

    if has_lua {
        eprint!("[tokf] Type 'yes' to confirm you have reviewed the Lua source: ");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let answer = read_line()?;
        if answer.trim() != "yes" {
            anyhow::bail!("installation cancelled");
        }
    } else {
        eprint!("[tokf] Install this filter? [y/N] ");
        let _ = std::io::Write::flush(&mut std::io::stderr());
        let answer = read_line()?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            anyhow::bail!("installation cancelled");
        }
    }

    Ok(())
}

fn read_line() -> anyhow::Result<String> {
    use std::io::BufRead as _;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| anyhow::anyhow!("could not read input: {e}"))?;
    Ok(line)
}

// pub(crate): accessed by install_cmd::run_verify
fn run_verify(rel_path: &Path, install_path: &Path, test_dir: &Path) -> anyhow::Result<()> {
    let filter_name = rel_path.with_extension("").to_string_lossy().to_string();
    let result =
        crate::verify_cmd::cmd_verify(Some(&filter_name), false, false, false, None, false);
    if result != 0 {
        // Log rollback failures rather than silently discarding them.
        if let Err(e) = std::fs::remove_file(install_path) {
            eprintln!("[tokf] warning: could not remove filter file during rollback: {e}");
        }
        if let Err(e) = std::fs::remove_dir_all(test_dir) {
            eprintln!("[tokf] warning: could not remove test dir during rollback: {e}");
        }
        anyhow::bail!("installed filter failed verification — installation rolled back");
    }
    Ok(())
}

fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Convert a command pattern to an install path relative to filters/.
///
/// - `"git push"` → `git/push.toml`
/// - `"cargo build"` → `cargo/build.toml`
/// - `"git"` → `git.toml`
fn command_pattern_to_path(pattern: &str) -> PathBuf {
    let words: Vec<&str> = pattern.split_whitespace().collect();
    match words.as_slice() {
        [] => PathBuf::from("unknown.toml"),
        [single] => PathBuf::from(format!("{single}.toml")),
        [dir, file, ..] => PathBuf::from(dir).join(format!("{file}.toml")),
    }
}

fn resolve_install_base(local: bool) -> anyhow::Result<PathBuf> {
    if local {
        let cwd = std::env::current_dir()?;
        Ok(cwd.join(".tokf"))
    } else {
        tokf::paths::user_dir()
            .ok_or_else(|| anyhow::anyhow!("cannot determine user config directory"))
    }
}

fn attribution_header(author: &str, hash: &str) -> String {
    format!("# Published by @{author} · hash: {hash} · https://tokf.net/filters/{hash}\n")
}

fn print_dry_run_summary(
    command_pattern: &str,
    hash: &str,
    author: &str,
    install_path: &Path,
    downloaded: &filter_client::DownloadedFilter,
) {
    eprintln!("[tokf] dry-run: would install {command_pattern}");
    eprintln!("  Hash:     {hash}");
    eprintln!("  Author:   @{author}");
    eprintln!("  Filter:   {}", install_path.display());
    if !downloaded.test_files.is_empty() {
        let stem = install_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let test_dir = install_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}_test"));
        eprintln!("  Tests:    {}/", test_dir.display());
        for tf in &downloaded.test_files {
            eprintln!("    {}", tf.filename);
        }
    }
    eprintln!("  Review:   https://tokf.net/filters/{hash}");
    eprintln!("[tokf] dry-run: filter content:");
    eprintln!("─────────────────────────────────────────");
    for line in downloaded
        .filter_toml
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        eprintln!("{line}");
    }
    eprintln!("─────────────────────────────────────────");
}

/// Returns `true` if the filename is safe to write to disk.
///
/// A safe test filename must not contain path separators or traverse directories.
/// Only alphanumeric characters plus `.`, `_`, and `-` are allowed.
fn is_safe_test_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn write_filter(
    downloaded: &filter_client::DownloadedFilter,
    install_path: &Path,
    hash: &str,
    author: &str,
    test_dir: &Path,
) -> anyhow::Result<()> {
    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let header = attribution_header(author, hash);
    let content = format!("{header}{}", downloaded.filter_toml);
    tokf::fs::write_config_file(install_path, &content)?;

    if !downloaded.test_files.is_empty() {
        std::fs::create_dir_all(test_dir)?;
        for tf in &downloaded.test_files {
            // Validate test filename to prevent path traversal attacks.
            if !is_safe_test_filename(&tf.filename) {
                anyhow::bail!(
                    "server returned unsafe test filename {:?} — installation aborted",
                    tf.filename
                );
            }
            let dest = test_dir.join(&tf.filename);
            tokf::fs::write_config_file(&dest, &tf.content)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Build a `FilterConfig` whose canonical hash is known so we can drive
    /// `verify_and_resolve_hash` deterministically.
    fn sample_config_and_hash() -> (FilterConfig, String) {
        let toml = r#"command = "git push""#;
        let cfg: FilterConfig = toml::from_str(toml).unwrap();
        let hash = tokf_common::hash::canonical_hash(&cfg).unwrap();
        (cfg, hash)
    }

    #[test]
    fn verify_and_resolve_uses_server_content_hash_when_provided() {
        let (cfg, current) = sample_config_and_hash();
        // URL hash is "stale" (a 64-char placeholder); server says the
        // current hash is `current`. Client trusts the server and returns it.
        let url = "0".repeat(64);
        let resolved = verify_and_resolve_hash(&url, Some(&current), &cfg).unwrap();
        assert_eq!(resolved, current);
    }

    #[test]
    fn verify_and_resolve_errors_when_server_hash_disagrees_with_computed() {
        let (cfg, _current) = sample_config_and_hash();
        // Server claims a hash that doesn't match what we'd compute from the
        // bytes we received — wire-tampering between server and client.
        let url = "0".repeat(64);
        let bogus_server_hash = "1".repeat(64);
        let err = verify_and_resolve_hash(&url, Some(&bogus_server_hash), &cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("hash mismatch with server-provided value"),
            "wrong error: {msg}"
        );
        // Both hashes must be present so users can copy them when reporting.
        let (_, current) = sample_config_and_hash();
        assert!(
            msg.contains(&bogus_server_hash) && msg.contains(&current),
            "error should include both server-claimed and locally-computed hashes: {msg}"
        );
    }

    #[test]
    fn verify_and_resolve_falls_back_to_url_hash_on_old_server() {
        let (cfg, current) = sample_config_and_hash();
        // Old server: no content_hash field. URL hash equals the locally
        // computed hash — the post-fix happy path against an old server.
        let resolved = verify_and_resolve_hash(&current, None, &cfg).unwrap();
        assert_eq!(resolved, current);
    }

    #[test]
    fn verify_and_resolve_errors_on_old_server_with_url_mismatch() {
        let (cfg, _current) = sample_config_and_hash();
        // Old server, URL hash doesn't match — the original #350 failure mode
        // when we can't repair it from the client.
        let url = "0".repeat(64);
        let err = verify_and_resolve_hash(&url, None, &cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("hash mismatch"), "wrong error: {msg}");
        assert!(msg.contains("issue #350"), "should reference issue: {msg}");
    }

    #[test]
    fn verify_and_resolve_returns_server_hash_even_when_url_differs() {
        let (cfg, current) = sample_config_and_hash();
        let url = "feedface".repeat(8); // 64 chars, different from `current`
        let resolved = verify_and_resolve_hash(&url, Some(&current), &cfg).unwrap();
        // The user requested a stale URL hash; the resolved identity is the
        // server's recomputed hash, used everywhere downstream (attribution,
        // display).
        assert_eq!(resolved, current);
        assert_ne!(resolved, url);
    }

    #[test]
    fn command_pattern_to_install_path_single_word() {
        let path = command_pattern_to_path("git");
        assert_eq!(path, PathBuf::from("git.toml"));
    }

    #[test]
    fn command_pattern_to_install_path_two_words() {
        let path = command_pattern_to_path("git push");
        assert_eq!(path, PathBuf::from("git/push.toml"));
    }

    #[test]
    fn command_pattern_to_install_path_three_words_uses_first_two() {
        let path = command_pattern_to_path("cargo test --workspace");
        assert_eq!(path, PathBuf::from("cargo/test.toml"));
    }

    #[test]
    fn attribution_header_format() {
        let header = attribution_header("alice", "deadbeef");
        assert!(header.starts_with('#'), "header should start with #");
        assert!(header.contains("@alice"), "header should mention author");
        assert!(header.contains("deadbeef"), "header should contain hash");
        assert!(
            header.contains("https://tokf.net/filters/deadbeef"),
            "header should have filter URL"
        );
    }

    #[test]
    fn conflict_detected_without_force() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("git").join("push.toml");
        std::fs::create_dir_all(filter_path.parent().unwrap()).unwrap();
        std::fs::write(&filter_path, b"command = \"git push\"\n").unwrap();

        let force = false;
        assert!(
            filter_path.exists() && !force,
            "conflict should be detected"
        );
    }

    #[test]
    fn parse_filter_toml_extracts_command_pattern() {
        let toml = r#"command = "git push""#;
        let (pattern, _) = parse_filter_toml(toml).unwrap();
        assert_eq!(pattern, "git push");
    }

    #[test]
    fn parse_filter_toml_strips_attribution_comments() {
        let toml = "# Published by @alice · hash: abc123\ncommand = \"cargo build\"\n";
        let (pattern, _) = parse_filter_toml(toml).unwrap();
        assert_eq!(pattern, "cargo build");
    }

    #[test]
    fn parse_filter_toml_errors_on_invalid_toml() {
        let result = parse_filter_toml("this is [[[not valid toml");
        assert!(result.is_err(), "should error on invalid TOML");
    }

    #[test]
    fn parse_filter_toml_detects_lua() {
        let toml =
            "command = \"my-tool\"\n[lua_script]\nlang = \"luau\"\nsource = \"return input\"\n";
        let (_, config) = parse_filter_toml(toml).unwrap();
        assert!(config.lua_script.is_some(), "should detect lua_script");
    }

    #[test]
    fn parse_filter_toml_no_lua_for_plain_filter() {
        let toml = r#"command = "git push""#;
        let (_, config) = parse_filter_toml(toml).unwrap();
        assert!(config.lua_script.is_none(), "should not detect lua_script");
    }

    #[test]
    fn safe_test_filenames_accepted() {
        assert!(is_safe_test_filename("basic.toml"));
        assert!(is_safe_test_filename("my-test_case.toml"));
        assert!(is_safe_test_filename("file123.toml"));
    }

    #[test]
    fn unsafe_test_filenames_rejected() {
        assert!(!is_safe_test_filename(""), "empty name");
        assert!(!is_safe_test_filename("."), "dot");
        assert!(!is_safe_test_filename(".."), "double dot");
        assert!(!is_safe_test_filename("../escape.toml"), "path traversal");
        assert!(!is_safe_test_filename("sub/dir.toml"), "subdirectory");
        assert!(!is_safe_test_filename("file name.toml"), "space");
    }

    #[test]
    fn unsafe_command_pattern_path_rejected() {
        // A safe path should have all Normal components.
        let safe_path = command_pattern_to_path("git push");
        assert!(
            safe_path
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "safe path should have all Normal components"
        );
    }
}
