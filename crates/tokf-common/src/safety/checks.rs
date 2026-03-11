use unicode_normalization::UnicodeNormalization as _;

use crate::config::types::FilterConfig;

use super::{SafetyCheck, SafetyWarning, WarningKind};

// ── Prompt injection ─────────────────────────────────────────────────────────

/// Case-insensitive prompt-injection patterns.
const INJECTION_PATTERNS: &[&str] = &[
    "ignore all previous instructions",
    "ignore previous instructions",
    "ignore prior instructions",
    "ignore above instructions",
    "disregard all previous instructions",
    "disregard previous instructions",
    "forget everything",
    "forget all previous",
    "you are now",
    "act as",
    "pretend to be",
    "system prompt",
    "reveal your",
    "new instructions",
    "override your",
];

/// NFKC-normalize and lowercase a string for more robust text matching.
///
/// This folds compatibility variants (e.g. fullwidth forms, ligatures) into
/// their canonical representations and normalizes case, but it does *not*
/// perform cross-script confusable/homoglyph mapping.
fn normalize_for_matching(s: &str) -> String {
    s.nfkc().collect::<String>().to_lowercase()
}

/// Detects prompt-injection patterns in templates and output pairs.
pub(super) struct PromptInjectionCheck;

impl SafetyCheck for PromptInjectionCheck {
    fn name(&self) -> &'static str {
        "prompt-injection"
    }

    fn check_config(&self, config: &FilterConfig) -> Vec<SafetyWarning> {
        let mut warnings = Vec::new();
        for (location, text) in extract_template_texts(config) {
            let normalized = normalize_for_matching(text);
            for pat in INJECTION_PATTERNS {
                if normalized.contains(pat) {
                    warnings.push(SafetyWarning {
                        kind: WarningKind::TemplateInjection,
                        message: format!("Template `{location}` contains prompt-injection pattern"),
                        detail: Some((*pat).to_string()),
                    });
                }
            }
        }
        warnings
    }

    fn check_output_pair(&self, raw: &str, filtered: &str) -> Vec<SafetyWarning> {
        let raw_normalized = normalize_for_matching(raw);
        let filtered_normalized = normalize_for_matching(filtered);
        let mut warnings = Vec::new();
        for pat in INJECTION_PATTERNS {
            if filtered_normalized.contains(pat) && !raw_normalized.contains(pat) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::OutputInjection,
                    message:
                        "Filtered output introduces prompt-injection pattern not present in raw input"
                            .to_string(),
                    detail: Some((*pat).to_string()),
                });
            }
        }
        warnings
    }
}

// ── Hidden Unicode ───────────────────────────────────────────────────────────

/// Hidden Unicode codepoints that could be used to smuggle text.
const HIDDEN_CODEPOINTS: &[char] = &[
    '\u{200B}', // zero-width space
    '\u{200C}', // zero-width non-joiner
    '\u{200D}', // zero-width joiner
    '\u{200E}', // left-to-right mark
    '\u{200F}', // right-to-left mark
    '\u{202A}', // left-to-right embedding
    '\u{202B}', // right-to-left embedding
    '\u{202C}', // pop directional formatting
    '\u{202D}', // left-to-right override
    '\u{202E}', // right-to-left override
    '\u{2060}', // word joiner
    '\u{2061}', // function application
    '\u{2062}', // invisible times
    '\u{2063}', // invisible separator
    '\u{2064}', // invisible plus
    '\u{FEFF}', // zero-width no-break space (BOM)
];

fn find_hidden_unicode(text: &str) -> Vec<char> {
    HIDDEN_CODEPOINTS
        .iter()
        .filter(|&&c| text.contains(c))
        .copied()
        .collect()
}

/// Detects hidden Unicode characters (zero-width, RTL overrides, etc.).
pub(super) struct HiddenUnicodeCheck;

impl SafetyCheck for HiddenUnicodeCheck {
    fn name(&self) -> &'static str {
        "hidden-unicode"
    }

    fn check_config(&self, config: &FilterConfig) -> Vec<SafetyWarning> {
        let mut warnings = Vec::new();
        for (location, text) in extract_template_texts(config) {
            for c in find_hidden_unicode(text) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::HiddenUnicode,
                    message: format!(
                        "Template `{location}` contains hidden Unicode character U+{:04X}",
                        c as u32
                    ),
                    detail: Some(format!("U+{:04X}", c as u32)),
                });
            }
        }
        for pattern in config.command.patterns() {
            for c in find_hidden_unicode(pattern) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::HiddenUnicode,
                    message: format!(
                        "Command pattern contains hidden Unicode character U+{:04X}",
                        c as u32
                    ),
                    detail: Some(format!("U+{:04X}", c as u32)),
                });
            }
        }
        for prefix in &config.passthrough_args {
            for c in find_hidden_unicode(prefix) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::HiddenUnicode,
                    message: format!(
                        "passthrough_args prefix contains hidden Unicode character U+{:04X}",
                        c as u32
                    ),
                    detail: Some(format!("U+{:04X}", c as u32)),
                });
            }
        }
        warnings
    }

    fn check_output_pair(&self, raw: &str, filtered: &str) -> Vec<SafetyWarning> {
        let mut warnings = Vec::new();
        for &c in HIDDEN_CODEPOINTS {
            if filtered.contains(c) && !raw.contains(c) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::HiddenUnicode,
                    message: format!(
                        "Filtered output introduces hidden Unicode character U+{:04X}",
                        c as u32
                    ),
                    detail: Some(format!("U+{:04X}", c as u32)),
                });
            }
        }
        warnings
    }

    fn check_rewrite(&self, replace: &str) -> Vec<SafetyWarning> {
        let mut warnings = Vec::new();
        for c in find_hidden_unicode(replace) {
            warnings.push(SafetyWarning {
                kind: WarningKind::HiddenUnicode,
                message: format!(
                    "Rewrite replacement contains hidden Unicode character U+{:04X}",
                    c as u32
                ),
                detail: Some(format!("U+{:04X}", c as u32)),
            });
        }
        warnings
    }
}

// ── Shell injection ──────────────────────────────────────────────────────────

/// Shell metacharacter patterns that indicate possible injection in rewrite
/// replacement strings.
const SHELL_INJECTION_PATTERNS: &[&str] = &[
    "$(",  // command substitution
    "$((", // arithmetic expansion
    "`",   // backtick command substitution
    "|",   // pipe
    "&&",  // command chaining
    "||",  // command chaining
    ";",   // command separator
    ">",   // output redirection
    "<",   // input redirection
];

/// Known-safe rewrite templates. The entire replacement must match one of these
/// patterns (after stripping surrounding whitespace). Only exact matches are
/// considered safe — the allowlist never suppresses individual metacharacter
/// warnings for replacements that contain extra content.
const SAFE_REWRITE_TEMPLATES: &[&str] = &["tokf run {0}", "tokf run {args}", "tokf run {0} {args}"];

/// Check a string for shell metacharacter patterns. Returns matched patterns.
fn check_shell_string(s: &str) -> Vec<&'static str> {
    SHELL_INJECTION_PATTERNS
        .iter()
        .filter(|pat| s.contains(*pat))
        .copied()
        .collect()
}

/// Detects shell metacharacters in rewrite replacement strings and
/// shell-executed config fields (`run`, `step[].run`).
pub(super) struct ShellInjectionCheck;

impl SafetyCheck for ShellInjectionCheck {
    fn name(&self) -> &'static str {
        "shell-injection"
    }

    fn check_config(&self, config: &FilterConfig) -> Vec<SafetyWarning> {
        let mut warnings = Vec::new();
        if let Some(ref run) = config.run {
            for w in check_shell_string(run) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::ShellInjection,
                    message: format!("`run` contains shell metacharacter `{w}`"),
                    detail: Some(w.to_string()),
                });
            }
        }
        for (i, step) in config.step.iter().enumerate() {
            for w in check_shell_string(&step.run) {
                warnings.push(SafetyWarning {
                    kind: WarningKind::ShellInjection,
                    message: format!("`step[{i}].run` contains shell metacharacter `{w}`"),
                    detail: Some(w.to_string()),
                });
            }
        }
        warnings
    }

    fn check_rewrite(&self, replace: &str) -> Vec<SafetyWarning> {
        let trimmed = replace.trim();
        // If the entire replacement matches a known-safe template, skip all checks.
        if SAFE_REWRITE_TEMPLATES.contains(&trimmed) {
            return vec![];
        }

        check_shell_string(replace)
            .into_iter()
            .map(|pat| SafetyWarning {
                kind: WarningKind::ShellInjection,
                message: format!("Rewrite replacement contains shell metacharacter `{pat}`"),
                detail: Some(pat.to_string()),
            })
            .collect()
    }
}

// ── Shared helpers ───────────────────────────────────────────────────────────

/// Extract all template text locations from a filter config for scanning.
///
/// Returns `(location_label, text)` pairs from every config field that can
/// contribute text to the final filtered output.
pub(super) fn extract_template_texts(config: &FilterConfig) -> Vec<(&'static str, &str)> {
    let mut t = Vec::new();

    // Branch outputs
    if let Some(ref b) = config.on_success
        && let Some(ref o) = b.output
    {
        t.push(("on_success.output", o.as_str()));
    }
    if let Some(ref b) = config.on_failure
        && let Some(ref o) = b.output
    {
        t.push(("on_failure.output", o.as_str()));
    }

    // Branch-level extract outputs
    if let Some(ref b) = config.on_success
        && let Some(ref e) = b.extract
    {
        t.push(("on_success.extract.output", e.output.as_str()));
    }
    if let Some(ref b) = config.on_failure
        && let Some(ref e) = b.extract
    {
        t.push(("on_failure.extract.output", e.output.as_str()));
    }

    // Match-output rules
    for rule in &config.match_output {
        t.push(("match_output.output", rule.output.as_str()));
    }

    // Top-level extract
    if let Some(ref e) = config.extract {
        t.push(("extract.output", e.output.as_str()));
    }

    // Replace rules
    for r in &config.replace {
        t.push(("replace.output", r.output.as_str()));
    }

    // Output formatting
    if let Some(ref o) = config.output {
        if let Some(ref f) = o.format {
            t.push(("output.format", f.as_str()));
        }
        if let Some(ref g) = o.group_counts_format {
            t.push(("output.group_counts_format", g.as_str()));
        }
        if let Some(ref e) = o.empty {
            t.push(("output.empty", e.as_str()));
        }
    }

    t
}
