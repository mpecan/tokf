// Types live in tokf-common; re-export for backward compatibility.
pub use tokf_common::config::types::*;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::literal_string_with_formatting_args
)]
mod tests {
    use super::*;

    fn load_filter(name: &str) -> FilterConfig {
        let path = format!("{}/filters/{name}", env!("CARGO_MANIFEST_DIR"));
        let content = std::fs::read_to_string(&path).unwrap();
        toml::from_str(&content).unwrap()
    }

    // --- CommandPattern deserialization ---

    #[test]
    fn test_command_pattern_single() {
        let cfg: FilterConfig = toml::from_str(r#"command = "git push""#).unwrap();
        assert_eq!(cfg.command, CommandPattern::Single("git push".to_string()));
        assert_eq!(cfg.command.first(), "git push");
        assert_eq!(cfg.command.patterns(), &["git push".to_string()]);
    }

    #[test]
    fn test_command_pattern_multiple() {
        let cfg: FilterConfig = toml::from_str(r#"command = ["pnpm test", "npm test"]"#).unwrap();
        assert_eq!(
            cfg.command,
            CommandPattern::Multiple(vec!["pnpm test".to_string(), "npm test".to_string()])
        );
        assert_eq!(cfg.command.first(), "pnpm test");
        assert_eq!(
            cfg.command.patterns(),
            &["pnpm test".to_string(), "npm test".to_string()]
        );
    }

    #[test]
    fn test_command_pattern_wildcard() {
        let cfg: FilterConfig = toml::from_str(r#"command = "npm run *""#).unwrap();
        assert_eq!(cfg.command.first(), "npm run *");
    }

    // --- Stdlib filter deserialization ---

    #[test]
    fn test_deserialize_git_push() {
        let cfg = load_filter("git/push.toml");

        assert_eq!(cfg.command.first(), "git push");
        assert_eq!(cfg.match_output.len(), 2);
        assert_eq!(cfg.match_output[0].contains, "Everything up-to-date");
        assert_eq!(cfg.match_output[1].contains, "non-fast-forward");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.skip.len(), 8);
        assert!(success.skip[0].starts_with("^Enumerating"));

        let extract = success.extract.unwrap();
        assert!(extract.pattern.contains("->"));
        assert_eq!(extract.output, "ok \u{2713} {2}");

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.skip.len(), 4);
        assert_eq!(failure.tail, Some(10));
    }

    #[test]
    fn test_deserialize_git_status() {
        let cfg = load_filter("git/status.toml");

        assert_eq!(cfg.command.first(), "git status");
        assert_eq!(cfg.run.as_deref(), Some("git status --porcelain -b"));

        let parse = cfg.parse.unwrap();
        let branch = parse.branch.unwrap();
        assert_eq!(branch.line, 1);
        assert_eq!(branch.output, "{1}");

        let group = parse.group.unwrap();
        assert!(group.labels.contains_key("??"));
        assert_eq!(group.labels.get("M ").unwrap(), "modified");

        let output = cfg.output.unwrap();
        assert!(output.format.unwrap().contains("{branch}"));
        assert_eq!(
            output.group_counts_format.as_deref(),
            Some("  {label}: {count}")
        );
        assert_eq!(
            output.empty.as_deref(),
            Some("clean \u{2014} nothing to commit")
        );
    }

    #[test]
    fn test_deserialize_cargo_test() {
        let cfg = load_filter("cargo/test.toml");

        assert_eq!(cfg.command.first(), "cargo test");
        assert!(!cfg.skip.is_empty());
        assert!(cfg.skip.iter().any(|s| s.contains("Compiling")));

        assert_eq!(cfg.section.len(), 3);
        assert_eq!(cfg.section[0].name.as_deref(), Some("failures"));
        assert_eq!(cfg.section[0].collect_as.as_deref(), Some("failure_blocks"));
        assert_eq!(cfg.section[1].name.as_deref(), Some("failure_names"));
        assert_eq!(cfg.section[2].name.as_deref(), Some("summary"));

        let success = cfg.on_success.unwrap();
        let agg = success.aggregate.unwrap();
        assert_eq!(agg.from, "summary_lines");
        assert_eq!(agg.sum.as_deref(), Some("passed"));
        assert_eq!(agg.count_as.as_deref(), Some("suites"));
        assert!(success.output.unwrap().contains("{passed}"));

        let failure = cfg.on_failure.unwrap();
        assert!(failure.output.unwrap().contains("FAILURES"));

        let fallback = cfg.fallback.unwrap();
        assert_eq!(fallback.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_add() {
        let cfg = load_filter("git/add.toml");

        assert_eq!(cfg.command.first(), "git add");
        assert_eq!(cfg.match_output.len(), 1);
        assert_eq!(cfg.match_output[0].contains, "fatal:");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("ok \u{2713}"));

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_commit() {
        let cfg = load_filter("git/commit.toml");

        assert_eq!(cfg.command.first(), "git commit");

        let success = cfg.on_success.unwrap();
        let extract = success.extract.unwrap();
        assert!(extract.pattern.contains("\\w+"));
        assert_eq!(extract.output, "ok \u{2713} {2}");

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    #[test]
    fn test_deserialize_git_log() {
        let cfg = load_filter("git/log.toml");

        assert_eq!(cfg.command.first(), "git log");

        let run = cfg.run.unwrap();
        assert!(run.contains("{args}"));
        assert!(run.contains("--oneline"));

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("{output}"));
    }

    #[test]
    fn test_deserialize_git_diff() {
        let cfg = load_filter("git/diff.toml");

        assert_eq!(cfg.command.first(), "git diff");

        let run = cfg.run.unwrap();
        assert!(run.contains("--stat"));
        assert!(run.contains("{args}"));

        assert_eq!(cfg.match_output.len(), 1);
        assert_eq!(cfg.match_output[0].contains, "fatal:");

        let success = cfg.on_success.unwrap();
        assert_eq!(success.output.as_deref(), Some("{output}"));

        let failure = cfg.on_failure.unwrap();
        assert_eq!(failure.tail, Some(5));
    }

    // --- Minimal / defaults ---

    #[test]
    fn test_minimal_config_only_command() {
        let cfg: FilterConfig = toml::from_str(r#"command = "echo""#).unwrap();

        assert_eq!(cfg.command.first(), "echo");
        assert_eq!(cfg.run, None);
        assert!(cfg.skip.is_empty());
        assert!(cfg.keep.is_empty());
        assert!(cfg.step.is_empty());
        assert_eq!(cfg.extract, None);
        assert!(cfg.match_output.is_empty());
        assert!(cfg.section.is_empty());
        assert_eq!(cfg.on_success, None);
        assert_eq!(cfg.on_failure, None);
        assert_eq!(cfg.parse, None);
        assert_eq!(cfg.output, None);
        assert_eq!(cfg.fallback, None);
        assert!(cfg.replace.is_empty());
        assert!(!cfg.dedup);
        assert_eq!(cfg.dedup_window, None);
        assert!(!cfg.strip_ansi);
        assert!(!cfg.trim_lines);
        assert!(!cfg.strip_empty_lines);
        assert!(!cfg.collapse_empty_lines);
        assert_eq!(cfg.lua_script, None);
        assert!(cfg.variant.is_empty());
    }

    // --- Variant deserialization ---

    #[test]
    fn test_variant_with_file_detection() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = ["npm test", "pnpm test"]

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts", "vitest.config.js"]
filter = "npm/test-vitest"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 1);
        assert_eq!(cfg.variant[0].name, "vitest");
        assert_eq!(
            cfg.variant[0].detect.files,
            vec!["vitest.config.ts", "vitest.config.js"]
        );
        assert_eq!(cfg.variant[0].detect.output_pattern, None);
        assert_eq!(cfg.variant[0].filter, "npm/test-vitest");
    }

    #[test]
    fn test_variant_with_output_pattern() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = "npm test"

[[variant]]
name = "mocha"
detect.output_pattern = "passing|failing|pending"
filter = "npm/test-mocha"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 1);
        assert_eq!(cfg.variant[0].name, "mocha");
        assert!(cfg.variant[0].detect.files.is_empty());
        assert_eq!(
            cfg.variant[0].detect.output_pattern.as_deref(),
            Some("passing|failing|pending")
        );
        assert_eq!(cfg.variant[0].filter, "npm/test-mocha");
    }

    #[test]
    fn test_multiple_variants() {
        let cfg: FilterConfig = toml::from_str(
            r#"
command = "npm test"

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts"]
filter = "npm/test-vitest"

[[variant]]
name = "jest"
detect.files = ["jest.config.js"]
filter = "npm/test-jest"

[[variant]]
name = "mocha"
detect.output_pattern = "passing|failing"
filter = "npm/test-mocha"
"#,
        )
        .unwrap();

        assert_eq!(cfg.variant.len(), 3);
        assert_eq!(cfg.variant[0].name, "vitest");
        assert_eq!(cfg.variant[1].name, "jest");
        assert_eq!(cfg.variant[2].name, "mocha");
    }

    // --- Negative tests ---

    #[test]
    fn test_missing_command_field_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(r#"run = "echo hello""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_type_for_skip_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(
            r#"command = "echo"
skip = "not-an-array""#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_type_for_tail_fails() {
        let result: Result<FilterConfig, _> = toml::from_str(
            r#"command = "echo"
[on_success]
tail = "five""#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_toml_fails() {
        let result: Result<FilterConfig, _> = toml::from_str("command = [unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_toml_fails() {
        let result: Result<FilterConfig, _> = toml::from_str("");
        assert!(result.is_err());
    }
}
