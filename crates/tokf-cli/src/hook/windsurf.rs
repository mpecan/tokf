use std::path::{Path, PathBuf};

use anyhow::Context;

use super::instructions;

/// Install the Windsurf rules file.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    let rules_path = if global {
        global_rules_path()?
    } else {
        PathBuf::from(".windsurf/rules/tokf.md")
    };
    install_to(&rules_path, global)
}

/// Core install logic with explicit path (testable).
pub(crate) fn install_to(rules_path: &Path, is_global: bool) -> anyhow::Result<()> {
    write_rules_file(rules_path, is_global)?;
    eprintln!(
        "[tokf] Windsurf rules installed to {}",
        rules_path.display()
    );
    if is_global {
        eprintln!("[tokf] Note: tokf section was appended to global rules file.");
    }
    Ok(())
}

fn global_rules_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".codeium/windsurf/memories/global_rules.md"))
}

fn write_rules_file(rules_path: &Path, is_global: bool) -> anyhow::Result<()> {
    if let Some(parent) = rules_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if is_global {
        // Append to global rules file (don't overwrite other rules)
        super::append_or_replace_section(rules_path, instructions::format_for_windsurf_global)
    } else {
        // Write standalone project rule file
        let content = instructions::format_for_windsurf();
        super::write_instruction_file(rules_path, &content)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_rules_file_project() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join(".windsurf/rules/tokf.md");

        install_to(&rules_path, false).unwrap();

        assert!(rules_path.exists());
        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("trigger: always_on"));
    }

    #[test]
    fn install_to_is_idempotent_project() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("tokf.md");

        install_to(&rules_path, false).unwrap();
        install_to(&rules_path, false).unwrap();

        assert!(rules_path.exists());
    }

    #[test]
    fn global_appends_to_existing_file() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("global_rules.md");
        std::fs::write(&rules_path, "# My existing rules\n").unwrap();

        install_to(&rules_path, true).unwrap();

        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(content.starts_with("# My existing rules\n"));
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("tokf run"));
        assert!(content.contains("<!-- tokf:end -->"));
    }

    #[test]
    fn global_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("global_rules.md");

        install_to(&rules_path, true).unwrap();
        install_to(&rules_path, true).unwrap();

        let content = std::fs::read_to_string(&rules_path).unwrap();
        let count = content.matches("<!-- tokf:start -->").count();
        assert_eq!(count, 1, "should have exactly one tokf section");
    }

    #[test]
    fn global_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("new_global_rules.md");

        install_to(&rules_path, true).unwrap();

        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("tokf run"));
    }
}
