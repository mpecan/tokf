pub use tokf_hook_types::{
    PermissionEngineType, PermissionsConfig, PipeConfig, RewriteConfig, RewriteRule, SkipConfig,
};

/// Options that control how the rewrite system generates `tokf run` commands.
#[derive(Debug, Clone, Default)]
pub struct RewriteOptions {
    /// When true, inject `--no-mask-exit-code` into generated `tokf run` commands.
    /// Used by shell/shim mode where the real exit code must propagate.
    pub no_mask_exit_code: bool,
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
        let toml_str = r"
[skip]
patterns = []
";
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let skip = config.skip.unwrap();
        assert!(skip.patterns.is_empty());
    }

    #[test]
    fn deserialize_pipe_section_defaults() {
        let toml_str = r"
[pipe]
";
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let pipe = config.pipe.unwrap();
        assert!(pipe.strip, "strip should default to true");
        assert!(!pipe.prefer_less, "prefer_less should default to false");
    }

    #[test]
    fn deserialize_pipe_strip_false() {
        let toml_str = r"
[pipe]
strip = false
";
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let pipe = config.pipe.unwrap();
        assert!(!pipe.strip);
    }

    #[test]
    fn deserialize_pipe_prefer_less_true() {
        let toml_str = r"
[pipe]
prefer_less = true
";
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let pipe = config.pipe.unwrap();
        assert!(pipe.strip, "strip should still default to true");
        assert!(pipe.prefer_less);
    }

    #[test]
    fn deserialize_pipe_both_fields() {
        let toml_str = r"
[pipe]
strip = false
prefer_less = true
";
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let pipe = config.pipe.unwrap();
        assert!(!pipe.strip);
        assert!(pipe.prefer_less);
    }

    #[test]
    fn deserialize_no_pipe_section() {
        let config: RewriteConfig = toml::from_str("").unwrap();
        assert!(config.pipe.is_none());
    }

    #[test]
    fn deserialize_pipe_with_other_sections() {
        let toml_str = r#"
[skip]
patterns = ["^my-tool"]

[pipe]
strip = false

[[rewrite]]
match = "^docker compose"
replace = "tokf run {0}"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        assert!(config.skip.is_some());
        assert!(!config.pipe.unwrap().strip);
        assert_eq!(config.rewrite.len(), 1);
    }

    #[test]
    fn deserialize_no_permissions_section() {
        let config: RewriteConfig = toml::from_str("").unwrap();
        assert!(config.permissions.is_none());
    }

    #[test]
    fn deserialize_permissions_builtin() {
        let toml_str = r#"
[permissions]
engine = "builtin"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let perms = config.permissions.unwrap();
        assert_eq!(perms.engine, PermissionEngineType::Builtin);
        assert!(perms.external.is_none());
    }

    #[test]
    fn deserialize_permissions_external() {
        let toml_str = r#"
[permissions]
engine = "external"

[permissions.external]
command = "dippy"
args = ["hook", "handle"]
timeout_ms = 3000
on_error = "builtin"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let perms = config.permissions.unwrap();
        assert_eq!(perms.engine, PermissionEngineType::External);
        let ext = perms.external.unwrap();
        assert_eq!(ext.command, "dippy");
        assert_eq!(ext.args, vec!["hook", "handle"]);
        assert_eq!(ext.timeout_ms, 3000);
        assert_eq!(
            ext.on_error,
            crate::hook::permission_engine::ErrorFallback::Builtin
        );
    }

    #[test]
    fn deserialize_permissions_external_defaults() {
        let toml_str = r#"
[permissions]
engine = "external"

[permissions.external]
command = "my-engine"
"#;
        let config: RewriteConfig = toml::from_str(toml_str).unwrap();
        let ext = config.permissions.unwrap().external.unwrap();
        assert_eq!(ext.command, "my-engine");
        assert!(ext.args.is_empty());
        assert_eq!(ext.timeout_ms, 5000);
        assert_eq!(
            ext.on_error,
            crate::hook::permission_engine::ErrorFallback::Ask
        );
    }
}
