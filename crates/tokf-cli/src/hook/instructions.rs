// Shared instruction content used by Tier 2 (rules-based) integrations.
// This avoids duplicating the Codex SKILL.md content across Cline, Windsurf,
// Copilot, and Aider installers.

/// Core instructions explaining what tokf does and how to use it.
const CORE_INSTRUCTIONS: &str = "\
# tokf — Token Output Filter

tokf compresses verbose command output so you receive concise, structured \
results instead of raw terminal dumps. It saves context tokens and makes \
output easier to reason about.

## Rule

When running a supported command, prefix it with `tokf run`:

```
tokf run <command>
```

**Examples:**

```sh
tokf run git status
tokf run cargo test
tokf run npm test
tokf run docker ps
```

## Supported commands

The following commands have built-in filters. Prefix these with `tokf run`:

- `git status`, `git diff`, `git log`, `git push`, `git add`, `git commit`, `git show`
- `cargo build`, `cargo test`, `cargo check`, `cargo clippy`, `cargo install`
- `npm test`, `npm run`, `yarn test`, `pnpm test`
- `docker build`, `docker compose`, `docker ps`, `docker images`
- `go build`, `go vet`
- `gradle build`, `gradle test`, `gradle dependencies`
- `gh pr view`, `gh pr list`, `gh pr checks`, `gh issue view`, `gh issue list`
- `kubectl get pods`
- `next build`
- `pnpm add`, `pnpm install`
- `prisma generate`
- `pytest`
- `tsc`
- `ls`

Commands not in this list pass through unchanged when prefixed with `tokf run`.

## Important rules

1. **Never double-prefix.** If a command already starts with `tokf run`, do not add it again.
2. **Arguments pass through.** Include all flags and arguments after the base command: \
`tokf run cargo test --release -- my_test`.
3. **Fail-safe.** If `tokf` is not installed or not on PATH, run the command without the prefix.
4. **Environment variables.** Place env vars before `tokf run`: \
`RUST_LOG=debug tokf run cargo test`.
5. **Pipes.** Do not add redundant filtering pipes (e.g. `| grep`, `| tail`, `| head`) \
after `tokf run` commands — tokf already compresses the output.
";

/// Format instructions for Cline (with YAML frontmatter, no path scoping).
pub fn format_for_cline() -> String {
    format!(
        "---\n\
         description: tokf — compress CLI output to save context tokens\n\
         globs:\n\
         alwaysApply: true\n\
         ---\n\n\
         {CORE_INSTRUCTIONS}"
    )
}

/// Format instructions for Windsurf (YAML frontmatter with trigger).
pub fn format_for_windsurf() -> String {
    format!(
        "---\n\
         trigger: always_on\n\
         description: tokf — compress CLI output to save context tokens\n\
         ---\n\n\
         {CORE_INSTRUCTIONS}"
    )
}

/// Format instructions for Windsurf global append (with tokf markers).
pub fn format_for_windsurf_global() -> String {
    let inner = format_for_windsurf();
    format!("<!-- tokf:start -->\n{inner}<!-- tokf:end -->")
}

/// Format instructions for GitHub Copilot (markdown with applyTo frontmatter).
pub fn format_for_copilot() -> String {
    format!(
        "---\n\
         applyTo: \"**\"\n\
         ---\n\n\
         {CORE_INSTRUCTIONS}"
    )
}

/// Format instructions as a section for appending to `copilot-instructions.md`.
pub fn format_for_copilot_append() -> String {
    format!("\n<!-- tokf:start -->\n{CORE_INSTRUCTIONS}<!-- tokf:end -->\n")
}

/// Format instructions for Aider (plain markdown for CONVENTIONS.md).
pub fn format_for_aider() -> String {
    format!("<!-- tokf:start -->\n{CORE_INSTRUCTIONS}<!-- tokf:end -->\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn cline_has_frontmatter() {
        let content = format_for_cline();
        assert!(content.starts_with("---\n"));
        assert!(content.contains("alwaysApply: true"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn windsurf_has_trigger() {
        let content = format_for_windsurf();
        assert!(content.contains("trigger: always_on"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn copilot_has_apply_to() {
        let content = format_for_copilot();
        assert!(content.contains("applyTo:"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn copilot_append_has_markers() {
        let content = format_for_copilot_append();
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("<!-- tokf:end -->"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn aider_has_markers() {
        let content = format_for_aider();
        assert!(content.contains("<!-- tokf:start -->"));
        assert!(content.contains("<!-- tokf:end -->"));
        assert!(content.contains("tokf run"));
    }

    #[test]
    fn all_formats_have_important_rules() {
        for content in [
            format_for_cline(),
            format_for_windsurf(),
            format_for_copilot(),
            format_for_aider(),
        ] {
            assert!(content.contains("Never double-prefix"), "missing rule 1");
            assert!(content.contains("Fail-safe"), "missing rule 3");
            assert!(content.contains("Supported commands"), "missing commands");
        }
    }
}
