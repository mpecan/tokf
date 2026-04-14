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
        let has_args = variant.detect.args_pattern.is_some();

        if !has_files && !has_output && !has_args {
            eprintln!(
                "[tokf] warning: variant '{}' has no detection criteria (no files, args_pattern, or output_pattern), skipping",
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

/// Resolve variants using args-pattern detection (Phase A.5).
///
/// Called after `remaining_args` is computed but before `should_passthrough()`.
/// Iterates `parent.variant` in declaration order. For each variant with
/// `detect.args_pattern`, compiles the regex and tests it against
/// `remaining_args.join(" ")`. On first match, looks up the variant's filter
/// and returns the replacement config.
///
/// Returns `None` when no args variant matches (parent config unchanged).
pub fn resolve_args_variants(
    parent: &FilterConfig,
    all_filters: &[ResolvedFilter],
    remaining_args: &[String],
    verbose: bool,
) -> Option<FilterConfig> {
    if remaining_args.is_empty() {
        return None;
    }
    let args_str = remaining_args.join(" ");
    for variant in &parent.variant {
        let Some(pattern) = &variant.detect.args_pattern else {
            continue;
        };
        let Ok(re) = Regex::new(pattern) else {
            eprintln!(
                "[tokf] warning: variant '{}' has invalid args_pattern '{}', skipping",
                variant.name, pattern
            );
            continue;
        };
        if re.is_match(&args_str) {
            if let Some(cfg) = lookup_filter_by_name(&variant.filter, all_filters) {
                if verbose {
                    eprintln!(
                        "[tokf] variant '{}' matched by args pattern, delegating to {}",
                        variant.name, variant.filter
                    );
                }
                return Some(cfg);
            }
            eprintln!(
                "[tokf] warning: variant '{}' references filter '{}' which was not found, skipping",
                variant.name, variant.filter
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
mod tests;
