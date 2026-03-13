use std::path::{Path, PathBuf};

use super::instructions;

/// Install the Aider conventions file.
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    if global {
        let conventions_path = global_conventions_path()?;
        write_conventions_file(&conventions_path)?;
        patch_aider_conf(&conventions_path)?;
        eprintln!(
            "[tokf] Aider conventions installed to {}",
            conventions_path.display()
        );
        eprintln!("[tokf] Updated ~/.aider.conf.yml to include the conventions file.");
        eprintln!("[tokf] Tip: you can also alias aider to auto-prefix commands with tokf run.");
    } else {
        let conventions_path = PathBuf::from("CONVENTIONS.md");
        install_to(&conventions_path)?;
    }
    Ok(())
}

/// Core install logic for project-local (testable).
pub(crate) fn install_to(conventions_path: &Path) -> anyhow::Result<()> {
    append_to_conventions(conventions_path)?;
    eprintln!(
        "[tokf] Aider conventions installed to {}",
        conventions_path.display()
    );
    eprintln!("[tokf] Aider will auto-discover CONVENTIONS.md in the project root.");
    Ok(())
}

fn global_conventions_path() -> anyhow::Result<PathBuf> {
    let user = crate::paths::user_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine config directory"))?;
    Ok(user.join("aider-conventions.md"))
}

fn write_conventions_file(path: &Path) -> anyhow::Result<()> {
    let content = instructions::format_for_aider();
    super::write_instruction_file(path, &content)
}

/// Append tokf section to CONVENTIONS.md, idempotent via markers.
fn append_to_conventions(path: &Path) -> anyhow::Result<()> {
    super::append_or_replace_section(path, instructions::format_for_aider)
}

/// Patch `~/.aider.conf.yml` to include the tokf conventions file in the `read:` list.
fn patch_aider_conf(conventions_path: &Path) -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
    let conf_path = home.join(".aider.conf.yml");

    let conventions_str = conventions_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("conventions path is not valid UTF-8"))?;

    let existing = match std::fs::read_to_string(&conf_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };

    // Check if already configured
    if existing.contains(conventions_str) {
        return Ok(());
    }

    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };

    let updated = format!("{existing}{separator}read:\n  - {conventions_str}\n");
    std::fs::write(&conf_path, updated)?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_conventions_file() {
        let dir = TempDir::new().unwrap();
        let conventions_path = dir.path().join("CONVENTIONS.md");

        install_to(&conventions_path).unwrap();

        assert!(conventions_path.exists());
        let content = std::fs::read_to_string(&conventions_path).unwrap();
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let conventions_path = dir.path().join("CONVENTIONS.md");

        install_to(&conventions_path).unwrap();
        install_to(&conventions_path).unwrap();

        let content = std::fs::read_to_string(&conventions_path).unwrap();
        let count = content.matches("<!-- tokf:start -->").count();
        assert_eq!(count, 1, "should have exactly one tokf section");
    }

    #[test]
    fn append_preserves_existing_content() {
        let dir = TempDir::new().unwrap();
        let conventions_path = dir.path().join("CONVENTIONS.md");
        std::fs::write(&conventions_path, "# Existing conventions\n").unwrap();

        install_to(&conventions_path).unwrap();

        let content = std::fs::read_to_string(&conventions_path).unwrap();
        assert!(content.starts_with("# Existing conventions\n"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn patch_aider_conf_creates_new_file() {
        let dir = TempDir::new().unwrap();
        // We can't test patch_aider_conf directly because it uses dirs::home_dir,
        // but we can test the underlying conventions writing
        let conventions_path = dir.path().join("tokf-conventions.md");
        write_conventions_file(&conventions_path).unwrap();

        let content = std::fs::read_to_string(&conventions_path).unwrap();
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn conventions_content_has_markers() {
        let content = instructions::format_for_aider();
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("<!-- tokf:end -->"));
    }
}
