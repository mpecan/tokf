use std::path::{Path, PathBuf};

use anyhow::Context;

const SKILL_MD: &str = include_str!("../../skills/codex-run/SKILL.md");
const DISCOVER_SKILL_MD: &str = include_str!("../../skills/codex-discover/SKILL.md");

struct CodexSkill {
    dir_name: &'static str,
    content: &'static str,
}

const CODEX_SKILLS: &[CodexSkill] = &[
    CodexSkill {
        dir_name: "tokf-run",
        content: SKILL_MD,
    },
    CodexSkill {
        dir_name: "tokf-discover",
        content: DISCOVER_SKILL_MD,
    },
];

/// Install Codex CLI skills (tokf-run + tokf-discover).
///
/// # Errors
///
/// Returns an error if the skill directory cannot be created or the skill file cannot be written.
pub fn install(global: bool) -> anyhow::Result<()> {
    let parent = if global {
        let home = dirs::home_dir().context("could not determine home directory")?;
        home.join(".agents/skills")
    } else {
        PathBuf::from(".agents/skills")
    };
    for skill in CODEX_SKILLS {
        write_skill_file(&parent.join(skill.dir_name), skill.content)?;
    }
    eprintln!("[tokf] Codex skills installed to {}", parent.display());
    eprintln!("[tokf] Codex will auto-discover the skills on next start.");
    Ok(())
}

#[cfg(test)]
fn install_to(skill_dir: &Path) -> anyhow::Result<()> {
    write_skill_file(skill_dir, SKILL_MD)?;
    eprintln!(
        "[tokf] Codex skill installed to {}",
        skill_dir.join("SKILL.md").display()
    );
    eprintln!("[tokf] Codex will auto-discover the skill on next start.");
    Ok(())
}

fn write_skill_file(skill_dir: &Path, content: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(skill_dir)
        .with_context(|| format!("failed to create skill dir: {}", skill_dir.display()))?;

    let skill_file = skill_dir.join("SKILL.md");
    std::fs::write(&skill_file, content)
        .with_context(|| format!("failed to write skill file: {}", skill_file.display()))?;
    eprintln!("[tokf] wrote {}", skill_file.display());

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_to_creates_skill_file() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();

        assert!(skill_dir.join("SKILL.md").exists());
    }

    #[test]
    fn install_to_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();
        install_to(&skill_dir).unwrap();

        // Only one file exists, no errors
        let entries: Vec<_> = std::fs::read_dir(&skill_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn skill_file_has_frontmatter() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tokf-run");

        install_to(&skill_dir).unwrap();

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(
            content.starts_with("---\n"),
            "SKILL.md should start with YAML frontmatter"
        );
        assert!(
            content.contains("name: tokf-run"),
            "SKILL.md frontmatter should include name: tokf-run"
        );
    }

    #[test]
    fn skill_file_has_description() {
        let content = SKILL_MD;
        assert!(
            content.contains("description:"),
            "SKILL.md frontmatter should include description"
        );
    }

    #[test]
    fn skill_file_mentions_tokf_run() {
        let content = SKILL_MD;
        assert!(
            content.contains("tokf run"),
            "SKILL.md should instruct using tokf run"
        );
    }

    #[test]
    fn skill_file_has_no_double_prefix_rule() {
        let content = SKILL_MD;
        assert!(
            content.contains("double-prefix"),
            "SKILL.md should warn against double-prefixing"
        );
    }

    #[test]
    fn skill_file_has_fail_safe_rule() {
        let content = SKILL_MD;
        assert!(
            content.contains("Fail-safe"),
            "SKILL.md should include fail-safe instruction"
        );
    }

    #[test]
    fn embedded_content_is_not_empty() {
        assert!(!SKILL_MD.is_empty(), "SKILL_MD should not be empty");
        assert!(
            !DISCOVER_SKILL_MD.is_empty(),
            "DISCOVER_SKILL_MD should not be empty"
        );
    }

    #[test]
    fn discover_skill_has_frontmatter() {
        assert!(
            DISCOVER_SKILL_MD.starts_with("---\n"),
            "discover SKILL.md should start with YAML frontmatter"
        );
        assert!(
            DISCOVER_SKILL_MD.contains("name: tokf-discover"),
            "discover SKILL.md should include name: tokf-discover"
        );
    }
}
