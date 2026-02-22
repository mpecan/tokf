use serde::Deserialize;

/// User-provided overrides loaded from `rewrites.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RewriteConfig {
    /// Additional skip patterns (commands matching these are never rewritten).
    pub skip: Option<SkipConfig>,

    /// User-defined rewrite rules (checked before auto-generated filter rules).
    #[serde(default)]
    pub rewrite: Vec<RewriteRule>,
}

/// Extra skip patterns from user config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkipConfig {
    /// Regex patterns — if any matches the command, rewriting is skipped.
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// A single rewrite rule: match a command and replace it.
#[derive(Debug, Clone, Deserialize)]
pub struct RewriteRule {
    /// Regex pattern to match against the command string.
    #[serde(rename = "match")]
    pub match_pattern: String,

    /// Replacement template. Supports `{0}` (full match), `{1}`, `{2}`, etc.
    pub replace: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_config() {
        let toml_str = r#"
[skip]
patterns = ["^my-tool ", "^internal-"]

[[rewrite]]
match = "^docker compose"
replace = "tokf run {0}"

[[rewrite]]
match = "^kubectl (get|describe)"
replace = "tokf run {0}"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();

        let skip = config.skip.unwrap();
        assert_eq!(skip.patterns.len(), 2);
        assert_eq!(skip.patterns[0], "^my-tool ");
        assert_eq!(skip.patterns[1], "^internal-");

        assert_eq!(config.rewrite.len(), 2);
        assert_eq!(config.rewrite[0].match_pattern, "^docker compose");
        assert_eq!(config.rewrite[0].replace, "tokf run {0}");
        assert_eq!(config.rewrite[1].match_pattern, "^kubectl (get|describe)");
    }

    #[test]
    fn deserialize_skip_only() {
        let toml_str = r#"
[skip]
patterns = ["^nope"]
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        assert!(config.skip.is_some());
        assert!(config.rewrite.is_empty());
    }

    #[test]
    fn deserialize_rules_only() {
        let toml_str = r#"
[[rewrite]]
match = "^make"
replace = "tokf run {0}"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        assert!(config.skip.is_none());
        assert_eq!(config.rewrite.len(), 1);
    }

    #[test]
    fn deserialize_empty_config() {
        let config: RewriteConfig = toml::from_str("").unwrap();
        assert!(config.skip.is_none());
        assert!(config.rewrite.is_empty());
    }

    #[test]
    fn deserialize_empty_skip_patterns() {
        let toml_str = r#"
[skip]
patterns = []
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let skip = config.skip.unwrap();
        assert!(skip.patterns.is_empty());
    }
}
