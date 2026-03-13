use std::path::{Path, PathBuf};

const SKILL_MD: &str = include_str!("../skills/tokf-filter/SKILL.md");
const STEP_REFERENCE_MD: &str = include_str!("../skills/tokf-filter/references/step-reference.md");
const EXAMPLES_TOML: &str = include_str!("../skills/tokf-filter/references/examples.toml");
const DISCOVER_SKILL_MD: &str = include_str!("../skills/tokf-discover/SKILL.md");

struct SkillFile {
    /// Relative path within the skill's directory.
    rel_path: &'static str,
    content: &'static str,
}

struct SkillBundle {
    dir_name: &'static str,
    files: &'static [SkillFile],
}

const FILTER_SKILL: SkillBundle = SkillBundle {
    dir_name: "tokf-filter",
    files: &[
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
    ],
};

const DISCOVER_SKILL: SkillBundle = SkillBundle {
    dir_name: "tokf-discover",
    files: &[SkillFile {
        rel_path: "SKILL.md",
        content: DISCOVER_SKILL_MD,
    }],
};

const ALL_SKILLS: &[&SkillBundle] = &[&FILTER_SKILL, &DISCOVER_SKILL];

/// Determine the skills parent directory.
fn skills_parent(global: bool) -> anyhow::Result<PathBuf> {
    if global {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?;
        Ok(home.join(".claude/skills"))
    } else {
        let cwd = std::env::current_dir()?;
        Ok(cwd.join(".claude/skills"))
    }
}

/// Install all skill bundles (tokf-filter + tokf-discover).
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn install(global: bool) -> anyhow::Result<()> {
    let parent = skills_parent(global)?;
    for bundle in ALL_SKILLS {
        install_bundle_to(&parent.join(bundle.dir_name), bundle)?;
    }
    Ok(())
}

/// Core install logic for a single bundle with an explicit base path (testable).
fn install_bundle_to(base: &Path, bundle: &SkillBundle) -> anyhow::Result<()> {
    for file in bundle.files {
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

/// Install just the filter skill to a specific path (used by tests).
#[cfg(test)]
fn install_to(base: &Path) -> anyhow::Result<()> {
    install_bundle_to(base, &FILTER_SKILL)
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
