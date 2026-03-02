use std::path::Path;

/// Parse filter bytes and return `(content_hash, command_pattern)`.
///
/// # Errors
///
/// Returns an error if the bytes are not valid UTF-8, the TOML is invalid, or
/// the canonical hash cannot be computed.
pub fn hash_filter(filter_bytes: &[u8]) -> anyhow::Result<(String, String)> {
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
///
/// # Errors
///
/// Returns an error if the TOML is invalid, the script file cannot be read,
/// or the script path escapes the filter directory.
pub fn inline_lua_script(filter_bytes: Vec<u8>, filter_path: &Path) -> anyhow::Result<Vec<u8>> {
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

/// Replace `fixture = "file.txt"` with `inline = "<file contents>"` in a test
/// TOML string. Fixture files are resolved relative to `test_dir`.
///
/// # Errors
///
/// Returns an error if the test TOML is invalid or a referenced fixture file
/// cannot be read.
pub fn resolve_fixtures_in_test(content: &str, test_dir: &Path) -> anyhow::Result<String> {
    let case: tokf_common::test_case::TestCase =
        toml::from_str(content).map_err(|e| anyhow::anyhow!("invalid test TOML: {e}"))?;

    let Some(fixture_name) = case.fixture else {
        return Ok(content.to_string());
    };

    if case.inline.is_some() {
        return Ok(content.to_string());
    }

    let fixture_path = test_dir.join(&fixture_name);
    let fixture_content = std::fs::read_to_string(&fixture_path)
        .map_err(|e| anyhow::anyhow!("cannot read fixture '{fixture_name}': {e}"))?;
    let fixture_content = fixture_content.trim_end();

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

    let insert_pos = result.find("[[expect]]").unwrap_or(result.len());
    let inline_value = format!("inline = '''\n{fixture_content}\n'''\n");
    result.insert_str(insert_pos, &inline_value);

    Ok(result)
}

/// Collect `.toml` test files from the `{stem}_test/` directory adjacent to
/// `filter_path`, resolving `fixture = "file.txt"` references to inline content.
///
/// Returns `(filename, bytes)` pairs sorted by filename. Non-TOML files are
/// excluded from the result.
///
/// # Errors
///
/// Returns an error if the test directory cannot be read or fixture resolution
/// fails for any test file.
pub fn collect_test_files_resolved(filter_path: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
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
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }

        let filename = entry.file_name().to_string_lossy().to_string();
        let content = std::fs::read_to_string(&path)?;
        let resolved = resolve_fixtures_in_test(&content, &test_dir)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

        files.push((filename, resolved.into_bytes()));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ── hash_filter ──────────────────────────────────────────────────────────

    #[test]
    fn hash_filter_returns_command_pattern() {
        let bytes = b"command = \"git push\"\n";
        let (hash, cmd) = hash_filter(bytes).unwrap();
        assert!(!hash.is_empty());
        assert_eq!(cmd, "git push");
    }

    // ── inline_lua_script ────────────────────────────────────────────────────

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

    // ── resolve_fixtures_in_test ─────────────────────────────────────────────

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
        std::fs::write(dir.path().join("output.txt"), "\\--- foo\n\\--- bar\n").unwrap();

        let toml = r#"name = "test"
fixture = "output.txt"
exit_code = 0

[[expect]]
contains = "foo"
"#;
        let resolved = resolve_fixtures_in_test(toml, dir.path()).unwrap();
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

    // ── collect_test_files_resolved ──────────────────────────────────────────

    #[test]
    fn collect_test_files_returns_empty_when_no_test_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        let files = collect_test_files_resolved(&filter_path).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn collect_test_files_from_adjacent_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        let test_dir = dir.path().join("my-filter_test");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::fs::write(
            test_dir.join("basic.toml"),
            "name = \"basic\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n",
        )
        .unwrap();
        std::fs::write(
            test_dir.join("edge.toml"),
            "name = \"edge\"\ninline = \"\"\n\n[[expect]]\nequals = \"\"\n",
        )
        .unwrap();

        let files = collect_test_files_resolved(&filter_path).unwrap();
        assert_eq!(files.len(), 2, "expected 2 test files");
        let names: std::collections::HashSet<_> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains("basic.toml"));
        assert!(names.contains("edge.toml"));
    }

    #[test]
    fn collect_test_files_resolved_inlines_fixtures() {
        let dir = tempfile::TempDir::new().unwrap();
        let filter_path = dir.path().join("my-filter.toml");
        std::fs::write(&filter_path, r#"command = "my-cmd""#).unwrap();

        let test_dir = dir.path().join("my-filter_test");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create a fixture file
        std::fs::write(test_dir.join("sample_output.txt"), "hello world\n").unwrap();

        // Create a test TOML referencing the fixture
        std::fs::write(
            test_dir.join("with_fixture.toml"),
            "name = \"fixture test\"\nfixture = \"sample_output.txt\"\n\n[[expect]]\ncontains = \"hello\"\n",
        )
        .unwrap();

        // Create a non-TOML file that should be excluded
        std::fs::write(test_dir.join("notes.txt"), "not a test").unwrap();

        let files = collect_test_files_resolved(&filter_path).unwrap();
        assert_eq!(files.len(), 1, "only .toml files should be included");

        let (name, bytes) = &files[0];
        assert_eq!(name, "with_fixture.toml");

        let content = std::str::from_utf8(bytes).unwrap();
        assert!(
            !content.contains("fixture ="),
            "fixture reference should be resolved"
        );
        assert!(
            content.contains("inline"),
            "should contain inline instead of fixture"
        );
        assert!(
            content.contains("hello world"),
            "should contain the fixture content"
        );
    }
}
