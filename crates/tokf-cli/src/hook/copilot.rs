use std::path::{Path, PathBuf};

use super::instructions;

/// Install the GitHub Copilot instructions.
///
/// Copilot only supports repo-level instructions, not global.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    if global {
        anyhow::bail!(
            "GitHub Copilot does not support global instruction files. \
             Use `--tool copilot` without `--global` to install project-level instructions."
        );
    }

    let instructions_dir = PathBuf::from(".github/instructions");
    let copilot_instructions = PathBuf::from(".github/copilot-instructions.md");
    install_to(&instructions_dir, &copilot_instructions)
}

/// Core install logic with explicit paths (testable).
pub(crate) fn install_to(
    instructions_dir: &Path,
    copilot_instructions_path: &Path,
) -> anyhow::Result<()> {
    // Write dedicated instruction file with applyTo frontmatter
    write_copilot_instruction_file(instructions_dir)?;

    // Also append to copilot-instructions.md for broader compatibility
    append_to_copilot_instructions(copilot_instructions_path)?;

    eprintln!(
        "[tokf] Copilot instructions installed to {}",
        instructions_dir.join("tokf.instructions.md").display()
    );
    eprintln!(
        "[tokf] Also appended to {}",
        copilot_instructions_path.display()
    );
    Ok(())
}

fn write_copilot_instruction_file(instructions_dir: &Path) -> anyhow::Result<()> {
    let file_path = instructions_dir.join("tokf.instructions.md");
    let content = instructions::format_for_copilot();
    super::write_instruction_file(&file_path, &content)
}

/// Append tokf section to copilot-instructions.md, idempotent via markers.
fn append_to_copilot_instructions(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    super::append_or_replace_section(path, instructions::format_for_copilot_append)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn global_install_fails() {
        let result = install(true);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("does not support global")
        );
    }

    #[test]
    fn install_to_creates_files() {
        let dir = TempDir::new().unwrap();
        let instructions_dir = dir.path().join(".github/instructions");
        let copilot_instructions = dir.path().join(".github/copilot-instructions.md");

        install_to(&instructions_dir, &copilot_instructions).unwrap();

        assert!(instructions_dir.join("tokf.instructions.md").exists());
        assert!(copilot_instructions.exists());
    }

    #[test]
    fn instruction_file_has_apply_to() {
        let dir = TempDir::new().unwrap();
        let instructions_dir = dir.path().join("instructions");
        let copilot_instructions = dir.path().join("copilot-instructions.md");

        install_to(&instructions_dir, &copilot_instructions).unwrap();

        let content =
            std::fs::read_to_string(instructions_dir.join("tokf.instructions.md")).unwrap();
        assert!(content.contains("applyTo:"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn append_preserves_existing_content() {
        let dir = TempDir::new().unwrap();
        let copilot_file = dir.path().join("copilot-instructions.md");
        std::fs::write(&copilot_file, "# Existing instructions\n").unwrap();

        let instructions_dir = dir.path().join("instructions");
        install_to(&instructions_dir, &copilot_file).unwrap();

        let content = std::fs::read_to_string(&copilot_file).unwrap();
        assert!(content.starts_with("# Existing instructions\n"));
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn append_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let copilot_file = dir.path().join("copilot-instructions.md");
        let instructions_dir = dir.path().join("instructions");

        install_to(&instructions_dir, &copilot_file).unwrap();
        install_to(&instructions_dir, &copilot_file).unwrap();

        let content = std::fs::read_to_string(&copilot_file).unwrap();
        let count = content.matches("<!-- tokf:start -->").count();
        assert_eq!(count, 1, "should have exactly one tokf section");
    }
}
