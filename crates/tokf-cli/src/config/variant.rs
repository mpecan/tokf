use std::path::Path;

use regex::Regex;

use super::ResolvedFilter;
use super::types::FilterConfig;

/// A variant whose detection is deferred to after command execution
/// (output-pattern matching).
#[derive(Debug, Clone)]
pub struct DeferredVariant {
    pub name: String,
    pub output_pattern: String,
    pub filter_name: String,
}

/// Result of Phase A (file-based) variant resolution.
#[derive(Debug)]
pub struct VariantResolution {
    /// The config to use (either a matched variant's config or the parent's).
    pub config: FilterConfig,
    /// Variants that need Phase B (output-pattern) resolution.
    pub output_variants: Vec<DeferredVariant>,
}

/// Resolve variants using file detection (Phase A).
///
/// Iterates `parent.variant` in declaration order. For each variant with
/// `detect.files`, checks if any listed file exists in `cwd`. On first file
/// match, looks up the variant's filter in `all_filters` and returns it.
///
/// Variants with only `detect.output_pattern` are collected as deferred for
/// Phase B (post-execution output matching).
pub fn resolve_variants(
    parent: &FilterConfig,
    all_filters: &[ResolvedFilter],
    cwd: &Path,
    verbose: bool,
) -> VariantResolution {
    let mut deferred = Vec::new();

    for variant in &parent.variant {
        let has_files = !variant.detect.files.is_empty();
        let has_output = variant.detect.output_pattern.is_some();

        if !has_files && !has_output {
            eprintln!(
                "[tokf] warning: variant '{}' has no detection criteria (no files or output_pattern), skipping",
                variant.name
            );
            continue;
        }

        if has_files {
            let file_match = variant.detect.files.iter().any(|f| cwd.join(f).exists());
            if file_match {
                if let Some(cfg) = lookup_filter_by_name(&variant.filter, all_filters) {
                    if verbose {
                        eprintln!(
                            "[tokf] variant '{}' matched by file detection, delegating to {}",
                            variant.name, variant.filter
                        );
                    }
                    return VariantResolution {
                        config: cfg,
                        output_variants: vec![],
                    };
                }
                eprintln!(
                    "[tokf] warning: variant '{}' references filter '{}' which was not found, skipping",
                    variant.name, variant.filter
                );
            }
            // File variant didn't match; if it also has an output pattern, defer it
            if has_output {
                deferred.push(DeferredVariant {
                    name: variant.name.clone(),
                    output_pattern: variant.detect.output_pattern.clone().unwrap_or_default(),
                    filter_name: variant.filter.clone(),
                });
            }
        } else if has_output {
            deferred.push(DeferredVariant {
                name: variant.name.clone(),
                output_pattern: variant.detect.output_pattern.clone().unwrap_or_default(),
                filter_name: variant.filter.clone(),
            });
        }
    }

    VariantResolution {
        config: parent.clone(),
        output_variants: deferred,
    }
}

/// Resolve deferred variants by matching output patterns (Phase B).
///
/// Returns the config of the first variant whose `output_pattern` regex
/// matches the command output, or `None` if no variant matches.
pub fn resolve_output_variants(
    variants: &[DeferredVariant],
    output: &str,
    all_filters: &[ResolvedFilter],
    verbose: bool,
) -> Option<FilterConfig> {
    for variant in variants {
        let Ok(re) = Regex::new(&variant.output_pattern) else {
            eprintln!(
                "[tokf] warning: variant '{}' has invalid output_pattern '{}', skipping",
                variant.name, variant.output_pattern
            );
            continue;
        };
        if re.is_match(output) {
            if let Some(cfg) = lookup_filter_by_name(&variant.filter_name, all_filters) {
                if verbose {
                    eprintln!(
                        "[tokf] variant '{}' matched by output pattern, delegating to {}",
                        variant.name, variant.filter_name
                    );
                }
                return Some(cfg);
            }
            eprintln!(
                "[tokf] warning: variant '{}' references filter '{}' which was not found, skipping",
                variant.name, variant.filter_name
            );
        }
    }
    None
}

/// Look up a filter by its display name (relative path without `.toml`).
pub fn lookup_filter_by_name(name: &str, filters: &[ResolvedFilter]) -> Option<FilterConfig> {
    filters
        .iter()
        .find(|f| f.matches_name(name))
        .map(|f| f.config.clone())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::config::types::*;

    fn make_filter_config(command: &str) -> FilterConfig {
        toml::from_str::<FilterConfig>(&format!("command = \"{command}\"")).unwrap()
    }

    fn make_resolved(name: &str, command: &str) -> ResolvedFilter {
        let config = make_filter_config(command);
        let hash = tokf_common::hash::canonical_hash(&config).unwrap_or_default();
        ResolvedFilter {
            config,
            hash,
            source_path: PathBuf::from(format!("<built-in>/{name}.toml")),
            relative_path: PathBuf::from(format!("{name}.toml")),
            priority: crate::config::STDLIB_PRIORITY,
        }
    }

    fn make_parent_with_variants(variants: Vec<Variant>) -> FilterConfig {
        let mut cfg = make_filter_config("npm test");
        cfg.variant = variants;
        cfg
    }

    fn make_variant(
        name: &str,
        files: Vec<&str>,
        output_pattern: Option<&str>,
        filter: &str,
    ) -> Variant {
        Variant {
            name: name.to_string(),
            detect: VariantDetect {
                files: files.into_iter().map(String::from).collect(),
                output_pattern: output_pattern.map(String::from),
            },
            filter: filter.to_string(),
        }
    }

    #[test]
    fn file_detection_resolves_variant() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("vitest.config.ts"), "").unwrap();

        let parent = make_parent_with_variants(vec![make_variant(
            "vitest",
            vec!["vitest.config.ts"],
            None,
            "npm/test-vitest",
        )]);
        let all_filters = vec![make_resolved("npm/test-vitest", "vitest")];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        assert_eq!(result.config.command.first(), "vitest");
        assert!(result.output_variants.is_empty());
    }

    #[test]
    fn no_file_match_falls_through_to_output_variants() {
        let tmp = TempDir::new().unwrap();
        // No vitest.config.ts created

        let parent = make_parent_with_variants(vec![
            make_variant("vitest", vec!["vitest.config.ts"], None, "npm/test-vitest"),
            make_variant("mocha", vec![], Some("passing|failing"), "npm/test-mocha"),
        ]);
        let all_filters = vec![
            make_resolved("npm/test-vitest", "vitest"),
            make_resolved("npm/test-mocha", "mocha"),
        ];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // Parent config unchanged
        assert_eq!(result.config.command.first(), "npm test");
        // Mocha is deferred for Phase B
        assert_eq!(result.output_variants.len(), 1);
        assert_eq!(result.output_variants[0].name, "mocha");
    }

    #[test]
    fn output_pattern_matches() {
        let all_filters = vec![make_resolved("npm/test-mocha", "mocha")];
        let deferred = vec![DeferredVariant {
            name: "mocha".to_string(),
            output_pattern: "passing|failing|pending".to_string(),
            filter_name: "npm/test-mocha".to_string(),
        }];

        let result =
            resolve_output_variants(&deferred, "  3 passing\n  1 failing", &all_filters, false);

        assert!(result.is_some());
        assert_eq!(result.unwrap().command.first(), "mocha");
    }

    #[test]
    fn output_pattern_no_match_returns_none() {
        let all_filters = vec![make_resolved("npm/test-mocha", "mocha")];
        let deferred = vec![DeferredVariant {
            name: "mocha".to_string(),
            output_pattern: "passing|failing|pending".to_string(),
            filter_name: "npm/test-mocha".to_string(),
        }];

        let result =
            resolve_output_variants(&deferred, "FAIL src/app.test.ts", &all_filters, false);

        assert!(result.is_none());
    }

    #[test]
    fn missing_variant_filter_skips() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("vitest.config.ts"), "").unwrap();

        let parent = make_parent_with_variants(vec![make_variant(
            "vitest",
            vec!["vitest.config.ts"],
            None,
            "npm/test-vitest",
        )]);
        // No filters available — the variant filter doesn't exist
        let all_filters: Vec<ResolvedFilter> = vec![];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // Falls back to parent
        assert_eq!(result.config.command.first(), "npm test");
    }

    #[test]
    fn empty_variants_returns_parent_unchanged() {
        let tmp = TempDir::new().unwrap();
        let parent = make_parent_with_variants(vec![]);
        let all_filters: Vec<ResolvedFilter> = vec![];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        assert_eq!(result.config.command.first(), "npm test");
        assert!(result.output_variants.is_empty());
    }

    #[test]
    fn first_file_match_wins() {
        let tmp = TempDir::new().unwrap();
        // Both config files exist
        std::fs::write(tmp.path().join("vitest.config.ts"), "").unwrap();
        std::fs::write(tmp.path().join("jest.config.js"), "").unwrap();

        let parent = make_parent_with_variants(vec![
            make_variant("vitest", vec!["vitest.config.ts"], None, "npm/test-vitest"),
            make_variant("jest", vec!["jest.config.js"], None, "npm/test-jest"),
        ]);
        let all_filters = vec![
            make_resolved("npm/test-vitest", "vitest"),
            make_resolved("npm/test-jest", "jest"),
        ];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // First variant wins
        assert_eq!(result.config.command.first(), "vitest");
    }

    #[test]
    fn lookup_filter_by_name_works() {
        let filters = vec![
            make_resolved("npm/test-vitest", "vitest"),
            make_resolved("npm/test-jest", "jest"),
        ];

        assert!(lookup_filter_by_name("npm/test-vitest", &filters).is_some());
        assert!(lookup_filter_by_name("npm/test-jest", &filters).is_some());
        assert!(lookup_filter_by_name("npm/test-mocha", &filters).is_none());
    }

    #[test]
    fn file_variant_with_output_pattern_defers_on_no_file_match() {
        let tmp = TempDir::new().unwrap();
        // No config file exists

        let parent = make_parent_with_variants(vec![make_variant(
            "vitest",
            vec!["vitest.config.ts"],
            Some("vitest|PASS|FAIL"),
            "npm/test-vitest",
        )]);
        let all_filters = vec![make_resolved("npm/test-vitest", "vitest")];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // Didn't match by file, deferred for output pattern
        assert_eq!(result.config.command.first(), "npm test");
        assert_eq!(result.output_variants.len(), 1);
        assert_eq!(result.output_variants[0].name, "vitest");
    }

    #[test]
    fn invalid_output_pattern_regex_skips_variant() {
        let all_filters = vec![make_resolved("npm/test-mocha", "mocha")];
        let deferred = vec![DeferredVariant {
            name: "bad-regex".to_string(),
            output_pattern: "[invalid(regex".to_string(),
            filter_name: "npm/test-mocha".to_string(),
        }];

        let result = resolve_output_variants(&deferred, "anything", &all_filters, false);

        assert!(result.is_none());
    }

    #[test]
    fn output_variant_filter_not_found_skips() {
        let all_filters: Vec<ResolvedFilter> = vec![];
        let deferred = vec![DeferredVariant {
            name: "mocha".to_string(),
            output_pattern: "passing".to_string(),
            filter_name: "npm/test-nonexistent".to_string(),
        }];

        let result = resolve_output_variants(&deferred, "3 passing", &all_filters, false);

        assert!(result.is_none());
    }

    #[test]
    fn multiple_deferred_variants_first_match_wins() {
        let all_filters = vec![
            make_resolved("npm/test-mocha", "mocha"),
            make_resolved("npm/test-tap", "tap"),
        ];
        let deferred = vec![
            DeferredVariant {
                name: "mocha".to_string(),
                output_pattern: "passing".to_string(),
                filter_name: "npm/test-mocha".to_string(),
            },
            DeferredVariant {
                name: "tap".to_string(),
                output_pattern: "ok \\d+".to_string(),
                filter_name: "npm/test-tap".to_string(),
            },
        ];

        // Both could match, but "passing" is checked first
        let result =
            resolve_output_variants(&deferred, "3 passing\nok 1 - test", &all_filters, false);

        assert!(result.is_some());
        assert_eq!(result.unwrap().command.first(), "mocha");
    }

    #[test]
    fn empty_variant_detect_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let parent = make_parent_with_variants(vec![Variant {
            name: "empty-detect".to_string(),
            detect: VariantDetect {
                files: vec![],
                output_pattern: None,
            },
            filter: "npm/test-whatever".to_string(),
        }]);
        let all_filters: Vec<ResolvedFilter> = vec![];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // Variant skipped, parent returned
        assert_eq!(result.config.command.first(), "npm test");
        assert!(result.output_variants.is_empty());
    }

    #[test]
    fn variant_resolution_debug_impl() {
        let tmp = TempDir::new().unwrap();
        let parent = make_parent_with_variants(vec![]);
        let all_filters: Vec<ResolvedFilter> = vec![];

        let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

        // Verify Debug derive works (Case 9)
        let debug_str = format!("{result:?}");
        assert!(debug_str.contains("VariantResolution"));
    }

    #[test]
    fn toml_ordering_variant_after_top_level_fields() {
        // Verify that [[variant]] entries placed after top-level fields
        // parse correctly (the TOML ordering requirement).
        let cfg: FilterConfig = toml::from_str(
            r#"
command = "npm test"
skip = ["^noise"]

[on_success]
output = "{output}"

[on_failure]
tail = 20

[[variant]]
name = "vitest"
detect.files = ["vitest.config.ts"]
filter = "npm/test-vitest"
"#,
        )
        .unwrap();

        // Skip patterns should be on the parent, not absorbed by variant
        assert_eq!(cfg.skip, vec!["^noise"]);
        assert_eq!(cfg.variant.len(), 1);
        assert_eq!(cfg.variant[0].name, "vitest");
        assert!(cfg.on_success.is_some());
        assert!(cfg.on_failure.is_some());
    }
}
