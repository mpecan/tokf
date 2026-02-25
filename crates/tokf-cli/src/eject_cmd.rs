use std::path::Path;

use tokf::config;

/// Entry point for the `tokf eject` subcommand.
pub fn cmd_eject(filter: &str, global: bool, no_cache: bool) -> i32 {
    match eject(filter, global, no_cache) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn eject(filter: &str, global: bool, no_cache: bool) -> anyhow::Result<()> {
    let target_base = if global {
        tokf::paths::user_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?
            .join("filters")
    } else {
        std::env::current_dir()?.join(".tokf/filters")
    };
    eject_to(filter, &target_base, no_cache)
}

/// Core eject logic with an explicit target base path (testable).
fn eject_to(filter: &str, target_base: &Path, no_cache: bool) -> anyhow::Result<()> {
    let filter_name = filter.strip_suffix(".toml").unwrap_or(filter);

    let search_dirs = config::default_search_dirs();
    let resolved = if no_cache {
        config::discover_all_filters(&search_dirs)?
    } else {
        config::cache::discover_with_cache(&search_dirs)?
    };

    let found = resolved.iter().find(|f| f.matches_name(filter_name));

    let resolved_filter =
        found.ok_or_else(|| anyhow::anyhow!("filter not found: {filter_name}"))?;

    // Compute target path and check for existing file
    let target_toml = target_base.join(&resolved_filter.relative_path);
    if target_toml.exists() {
        anyhow::bail!(
            "filter already exists at {}  — remove it first to re-eject",
            target_toml.display()
        );
    }

    // Copy the .toml file
    let toml_content = if resolved_filter.priority == u8::MAX {
        config::get_embedded_filter(&resolved_filter.relative_path)
            .ok_or_else(|| anyhow::anyhow!("embedded filter not readable"))?
            .to_string()
    } else {
        std::fs::read_to_string(&resolved_filter.source_path)?
    };

    if let Some(parent) = target_toml.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target_toml, &toml_content)?;
    eprintln!("[tokf] wrote {}", target_toml.display());

    // Copy the _test/ directory if present
    copy_test_suite(resolved_filter, target_base)?;

    eprintln!("[tokf] ejected {filter_name} to {}", target_base.display());
    Ok(())
}

/// Copy the `_test/` directory for a filter (if it exists) to the target base.
fn copy_test_suite(
    resolved_filter: &config::ResolvedFilter,
    target_base: &Path,
) -> anyhow::Result<()> {
    let stem = resolved_filter
        .relative_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let test_dir_name = format!("{stem}_test");
    let test_dir_relative = resolved_filter
        .relative_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(&test_dir_name);

    let wrote = if resolved_filter.priority == u8::MAX {
        copy_embedded_test_dir(&test_dir_relative, target_base)?
    } else {
        let source_test_dir = resolved_filter
            .source_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&test_dir_name);
        if source_test_dir.is_dir() {
            let target_test_dir = target_base.join(&test_dir_relative);
            copy_dir_flat(&source_test_dir, &target_test_dir)?;
            true
        } else {
            false
        }
    };

    if wrote {
        eprintln!("[tokf] copied test suite: {test_dir_name}/");
    }
    Ok(())
}

/// Copy embedded test directory files to the target base, returning true if any files were written.
fn copy_embedded_test_dir(test_dir_relative: &Path, target_base: &Path) -> anyhow::Result<bool> {
    let files = config::get_embedded_dir_files(test_dir_relative);
    if files.is_empty() {
        return Ok(false);
    }
    for (rel_path, content) in &files {
        let dest = target_base.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, content)?;
        eprintln!("[tokf] wrote {}", dest.display());
    }
    Ok(true)
}

/// Copy all files from `src_dir` into `dest_dir` (non-recursive, flat copy).
fn copy_dir_flat(src_dir: &Path, dest_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest_dir)?;
    let entries = std::fs::read_dir(src_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let dest = dest_dir.join(entry.file_name());
            std::fs::copy(&path, &dest)?;
            eprintln!("[tokf] wrote {}", dest.display());
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn eject_builtin_filter_writes_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("filters");

        // "cargo/build" is a known built-in filter
        eject_to("cargo/build", &target, true).unwrap();

        let toml_path = target.join("cargo/build.toml");
        assert!(toml_path.exists(), "toml file should be created");
        let content = std::fs::read_to_string(&toml_path).unwrap();
        assert!(
            content.contains("command"),
            "toml should contain a command field"
        );
    }

    #[test]
    fn eject_builtin_copies_test_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("filters");

        eject_to("cargo/build", &target, true).unwrap();

        let test_dir = target.join("cargo/build_test");
        assert!(test_dir.is_dir(), "test directory should be created");

        // build_test contains .toml and .txt fixture files
        let entries: Vec<_> = std::fs::read_dir(&test_dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(!entries.is_empty(), "test directory should contain files");
    }

    #[test]
    fn eject_refuses_if_target_exists() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("filters");

        // First eject should succeed
        eject_to("cargo/build", &target, true).unwrap();

        // Second eject should fail
        let result = eject_to("cargo/build", &target, true);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "error should mention 'already exists', got: {err}"
        );
    }

    #[test]
    fn eject_nonexistent_filter_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("filters");

        let result = eject_to("nonexistent/filter", &target, true);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "error should mention 'not found', got: {err}"
        );
    }

    #[test]
    fn eject_strips_toml_extension() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("filters");

        // Should work even with .toml extension
        eject_to("cargo/build.toml", &target, true).unwrap();

        let toml_path = target.join("cargo/build.toml");
        assert!(toml_path.exists());
    }
}
