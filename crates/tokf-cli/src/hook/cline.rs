use std::path::{Path, PathBuf};

use anyhow::Context;

use super::instructions;

/// Install the Cline rules file.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    let rules_path = if global {
        global_rules_path()?
    } else {
        PathBuf::from(".clinerules/tokf.md")
    };
    install_to(&rules_path)
}

/// Core install logic with explicit path (testable).
pub(crate) fn install_to(rules_path: &Path) -> anyhow::Result<()> {
    write_rules_file(rules_path)?;
    eprintln!("[tokf] Cline rules installed to {}", rules_path.display());
    eprintln!("[tokf] Cline will auto-discover the rules on next start.");
    Ok(())
}

fn global_rules_path() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join("Documents/Cline/Rules/tokf.md"))
}

fn write_rules_file(rules_path: &Path) -> anyhow::Result<()> {
    let content = instructions::format_for_cline();
    super::write_instruction_file(rules_path, &content)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_rules_file() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join(".clinerules/tokf.md");

        install_to(&rules_path).unwrap();

        assert!(rules_path.exists());
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join(".clinerules/tokf.md");

        install_to(&rules_path).unwrap();
        install_to(&rules_path).unwrap();

        assert!(rules_path.exists());
    }

    #[test]
    fn rules_file_has_frontmatter() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("tokf.md");

        install_to(&rules_path).unwrap();

        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("alwaysApply: true"));
    }

    #[test]
    fn rules_file_has_tokf_instructions() {
        let dir = TempDir::new().unwrap();
        let rules_path = dir.path().join("tokf.md");

        install_to(&rules_path).unwrap();

        let content = std::fs::read_to_string(&rules_path).unwrap();
        assert!(content.contains("tokf run"));
        assert!(content.contains("Never double-prefix"));
    }
}
