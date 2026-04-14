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
    args_pattern: Option<&str>,
    filter: &str,
) -> Variant {
    Variant {
        name: name.to_string(),
        detect: VariantDetect {
            files: files.into_iter().map(String::from).collect(),
            output_pattern: output_pattern.map(String::from),
            args_pattern: args_pattern.map(String::from),
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

    let parent = make_parent_with_variants(vec![
        make_variant(
            "vitest",
            vec!["vitest.config.ts"],
            None,
            None,
            "npm/test-vitest",
        ),
        make_variant(
            "mocha",
            vec![],
            Some("passing|failing"),
            None,
            "npm/test-mocha",
        ),
    ]);
    let all_filters = vec![
        make_resolved("npm/test-vitest", "vitest"),
        make_resolved("npm/test-mocha", "mocha"),
    ];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

    assert_eq!(result.config.command.first(), "npm test");
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

    let result = resolve_output_variants(&deferred, "FAIL src/app.test.ts", &all_filters, false);

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
        None,
        "npm/test-vitest",
    )]);
    let all_filters: Vec<ResolvedFilter> = vec![];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

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
    std::fs::write(tmp.path().join("vitest.config.ts"), "").unwrap();
    std::fs::write(tmp.path().join("jest.config.js"), "").unwrap();

    let parent = make_parent_with_variants(vec![
        make_variant(
            "vitest",
            vec!["vitest.config.ts"],
            None,
            None,
            "npm/test-vitest",
        ),
        make_variant("jest", vec!["jest.config.js"], None, None, "npm/test-jest"),
    ]);
    let all_filters = vec![
        make_resolved("npm/test-vitest", "vitest"),
        make_resolved("npm/test-jest", "jest"),
    ];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

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

    let parent = make_parent_with_variants(vec![make_variant(
        "vitest",
        vec!["vitest.config.ts"],
        Some("vitest|PASS|FAIL"),
        None,
        "npm/test-vitest",
    )]);
    let all_filters = vec![make_resolved("npm/test-vitest", "vitest")];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

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

    let result = resolve_output_variants(&deferred, "3 passing\nok 1 - test", &all_filters, false);

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
            args_pattern: None,
        },
        filter: "npm/test-whatever".to_string(),
    }]);
    let all_filters: Vec<ResolvedFilter> = vec![];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

    assert_eq!(result.config.command.first(), "npm test");
    assert!(result.output_variants.is_empty());
}

#[test]
fn variant_resolution_debug_impl() {
    let tmp = TempDir::new().unwrap();
    let parent = make_parent_with_variants(vec![]);
    let all_filters: Vec<ResolvedFilter> = vec![];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

    let debug_str = format!("{result:?}");
    assert!(debug_str.contains("VariantResolution"));
}

#[test]
fn toml_ordering_variant_after_top_level_fields() {
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

    assert_eq!(cfg.skip, vec!["^noise"]);
    assert_eq!(cfg.variant.len(), 1);
    assert_eq!(cfg.variant[0].name, "vitest");
    assert!(cfg.on_success.is_some());
    assert!(cfg.on_failure.is_some());
}

// --- args_pattern variant detection (Phase A.5) ---

#[test]
fn args_pattern_matches_variant() {
    let parent = make_parent_with_variants(vec![make_variant(
        "name-list",
        vec![],
        None,
        Some("--(name-only|name-status)"),
        "git/diff-name-list",
    )]);
    let all_filters = vec![make_resolved("git/diff-name-list", "git diff")];
    let args: Vec<String> = vec!["--name-only".into()];

    let result = resolve_args_variants(&parent, &all_filters, &args, false);

    assert!(result.is_some());
    assert_eq!(result.unwrap().command.first(), "git diff");
}

#[test]
fn args_pattern_no_match_returns_none() {
    let parent = make_parent_with_variants(vec![make_variant(
        "name-list",
        vec![],
        None,
        Some("--(name-only|name-status)"),
        "git/diff-name-list",
    )]);
    let all_filters = vec![make_resolved("git/diff-name-list", "git diff")];
    let args: Vec<String> = vec!["--stat".into()];

    let result = resolve_args_variants(&parent, &all_filters, &args, false);

    assert!(result.is_none());
}

#[test]
fn args_pattern_empty_args_returns_none() {
    let parent = make_parent_with_variants(vec![make_variant(
        "name-list",
        vec![],
        None,
        Some("--(name-only|name-status)"),
        "git/diff-name-list",
    )]);
    let all_filters = vec![make_resolved("git/diff-name-list", "git diff")];

    let result = resolve_args_variants(&parent, &all_filters, &[], false);

    assert!(result.is_none());
}

#[test]
fn args_pattern_invalid_regex_skips() {
    let parent = make_parent_with_variants(vec![make_variant(
        "bad-regex",
        vec![],
        None,
        Some("[invalid(regex"),
        "git/diff-name-list",
    )]);
    let all_filters = vec![make_resolved("git/diff-name-list", "git diff")];
    let args: Vec<String> = vec!["--name-only".into()];

    let result = resolve_args_variants(&parent, &all_filters, &args, false);

    assert!(result.is_none());
}

#[test]
fn args_pattern_first_match_wins() {
    let parent = make_parent_with_variants(vec![
        make_variant(
            "name-list",
            vec![],
            None,
            Some("--name-only"),
            "git/diff-name-list",
        ),
        make_variant(
            "name-status",
            vec![],
            None,
            Some("--name-status"),
            "git/diff-name-status",
        ),
    ]);
    let all_filters = vec![
        make_resolved("git/diff-name-list", "git diff --name-only"),
        make_resolved("git/diff-name-status", "git diff --name-status"),
    ];
    let args: Vec<String> = vec!["--name-only".into(), "--name-status".into()];

    let result = resolve_args_variants(&parent, &all_filters, &args, false);

    assert!(result.is_some());
    assert_eq!(result.unwrap().command.first(), "git diff --name-only");
}

#[test]
fn args_pattern_missing_filter_skips() {
    let parent = make_parent_with_variants(vec![make_variant(
        "name-list",
        vec![],
        None,
        Some("--name-only"),
        "git/diff-nonexistent",
    )]);
    let all_filters: Vec<ResolvedFilter> = vec![];
    let args: Vec<String> = vec!["--name-only".into()];

    let result = resolve_args_variants(&parent, &all_filters, &args, false);

    assert!(result.is_none());
}

#[test]
fn args_only_variant_not_deferred_in_file_resolution() {
    let tmp = TempDir::new().unwrap();
    let parent = make_parent_with_variants(vec![make_variant(
        "name-list",
        vec![],
        None,
        Some("--name-only"),
        "git/diff-name-list",
    )]);
    let all_filters: Vec<ResolvedFilter> = vec![];

    let result = resolve_variants(&parent, &all_filters, tmp.path(), false);

    assert!(result.output_variants.is_empty());
    assert_eq!(result.config.command.first(), "npm test");
}
