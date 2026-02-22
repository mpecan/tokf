use std::path::{Path, PathBuf};

const SKILL_MD: &str = include_str!("../../../.claude/skills/tokf-filter/SKILL.md");
const STEP_REFERENCE_MD: &str =
    include_str!("../../../.claude/skills/tokf-filter/references/step-reference.md");
const EXAMPLES_TOML: &str =
    include_str!("../../../.claude/skills/tokf-filter/references/examples.toml");

struct SkillFile {
    rel_path: &'static str,
    content: &'static str,
}

const SKILL_FILES: &[SkillFile] = &[
    SkillFile {
        rel_path: "SKILL.md",
        content: SKILL_MD,
    },
    SkillFile {
        rel_path: "references/step-reference.md",
        content: STEP_REFERENCE_MD,
    },
    SkillFile {
        rel_path: "references/examples.toml",
        content: EXAMPLES_TOML,
    },
];

/// Determine the target base directory for the skill files.
fn skill_base_dir(global: bool) -> anyhow::Result<PathBuf> {
    if global {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(home.join(".claude/skills/tokf-filter"))
    } else {
        let cwd = std::env::current_dir()?;
        Ok(cwd.join(".claude/skills/tokf-filter"))
    }
}

/// Install skill files to `~/.claude/skills/tokf-filter/` (global) or
/// `.claude/skills/tokf-filter/` in the current directory (project-local).
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    let base = skill_base_dir(global)?;
    install_to(&base)
}

/// Core install logic with an explicit base path (testable).
pub(crate) fn install_to(base: &Path) -> anyhow::Result<()> {
    for file in SKILL_FILES {
        let dest = base.join(file.rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.content)?;
        eprintln!("[tokf] wrote {}", dest.display());
    }
    eprintln!("[tokf] skill installed: {}", base.display());
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn install_to_creates_all_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().join("tokf-filter");

        install_to(&base).unwrap();

        assert!(base.join("SKILL.md").exists());
        assert!(base.join("references/step-reference.md").exists());
        assert!(base.join("references/examples.toml").exists());
    }

    #[test]
    fn install_to_skill_md_has_frontmatter() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().join("tokf-filter");

        install_to(&base).unwrap();

        let content = std::fs::read_to_string(base.join("SKILL.md")).unwrap();
        assert!(
            content.starts_with("---\n"),
            "SKILL.md should start with YAML frontmatter"
        );
        assert!(
            content.contains("name: tokf-filter"),
            "SKILL.md frontmatter should include name"
        );
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().join("tokf-filter");

        install_to(&base).unwrap();
        install_to(&base).unwrap();

        // All files still exist and are not corrupted
        let content = std::fs::read_to_string(base.join("SKILL.md")).unwrap();
        assert!(!content.is_empty());
    }

    #[test]
    fn install_to_references_dir_is_created() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = dir.path().join("nested/skill");

        install_to(&base).unwrap();

        assert!(base.join("references").is_dir());
    }

    #[test]
    fn embedded_content_matches_source_files() {
        assert!(!SKILL_MD.is_empty(), "SKILL_MD should not be empty");
        assert!(
            !STEP_REFERENCE_MD.is_empty(),
            "STEP_REFERENCE_MD should not be empty"
        );
        assert!(
            !EXAMPLES_TOML.is_empty(),
            "EXAMPLES_TOML should not be empty"
        );
    }
}
