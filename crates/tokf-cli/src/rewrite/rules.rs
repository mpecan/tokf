use regex::Regex;

use super::types::RewriteRule;

/// Built-in skip patterns that are always active.
/// - `^tokf ` prevents double-wrapping
/// - `<<` prevents rewriting heredocs
const BUILTIN_SKIP_PATTERNS: &[&str] = &["^tokf ", "<<"];

/// Check if a command should be skipped (not rewritten).
pub fn should_skip(command: &str, user_patterns: &[String]) -> bool {
    for pattern in BUILTIN_SKIP_PATTERNS {
        if let Ok(re) = Regex::new(pattern)
            && re.is_match(command)
        {
            return true;
        }
    }

    for pattern in user_patterns {
        match Regex::new(pattern) {
            Ok(re) if re.is_match(command) => return true,
            Err(e) => {
                eprintln!("[tokf] warning: invalid skip pattern \"{pattern}\": {e}");
            }
            _ => {}
        }
    }

    false
}

/// Apply the first matching rewrite rule. Returns the original command if none match.
pub fn apply_rules(rules: &[RewriteRule], command: &str) -> String {
    for rule in rules {
        let Ok(re) = Regex::new(&rule.match_pattern) else {
            continue;
        };

        if let Some(caps) = re.captures(command) {
            return interpolate_rewrite(&rule.replace, &caps, command);
        }
    }

    command.to_string()
}

/// Interpolate `{0}`, `{1}`, `{2}`, ... and `{rest}` in the replacement template.
fn interpolate_rewrite(template: &str, caps: &regex::Captures<'_>, full_input: &str) -> String {
    let mut result = template.to_string();

    // Handle the {rest} placeholder — text after the entire match
    let rest = &full_input[caps.get(0).map_or(full_input.len(), |m| m.end())..];
    let rest = rest.trim_start();
    #[allow(clippy::literal_string_with_formatting_args)]
    let rest_token = "{rest}";
    result = result.replace(rest_token, rest);

    // Handle numbered groups in reverse order (so {10} is replaced before {1})
    let max_group = caps.len().saturating_sub(1);
    for i in (0..=max_group).rev() {
        let placeholder = format!("{{{i}}}");
        let value = caps.get(i).map_or("", |m| m.as_str());
        result = result.replace(&placeholder, value);
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::trivial_regex)]
mod tests {
    use super::*;

    // --- should_skip ---

    #[test]
    fn skip_tokf_commands() {
        assert!(should_skip("tokf run git status", &[]));
        assert!(should_skip("tokf rewrite foo", &[]));
    }

    #[test]
    fn skip_heredocs() {
        assert!(should_skip("cat <<EOF", &[]));
        assert!(should_skip("bash -c 'cat <<EOF'", &[]));
    }

    #[test]
    fn skip_user_patterns() {
        let patterns = vec!["^my-internal".to_string()];
        assert!(should_skip("my-internal tool", &patterns));
        assert!(!should_skip("git status", &patterns));
    }

    #[test]
    fn skip_invalid_user_pattern_does_not_crash() {
        // Invalid regex should produce a warning but not skip or crash
        let patterns = vec!["[invalid".to_string()];
        assert!(!should_skip("git status", &patterns));
    }

    #[test]
    fn no_skip_normal_commands() {
        assert!(!should_skip("git status", &[]));
        assert!(!should_skip("cargo test", &[]));
        assert!(!should_skip("ls -la", &[]));
    }

    // --- apply_rules ---

    #[test]
    fn apply_rules_first_match_wins() {
        let rules = vec![
            RewriteRule {
                match_pattern: "^git status".to_string(),
                replace: "first {0}".to_string(),
            },
            RewriteRule {
                match_pattern: "^git".to_string(),
                replace: "second {0}".to_string(),
            },
        ];
        assert_eq!(apply_rules(&rules, "git status"), "first git status");
    }

    #[test]
    fn apply_rules_no_match_returns_original() {
        let rules = vec![RewriteRule {
            match_pattern: "^git".to_string(),
            replace: "tokf run {0}".to_string(),
        }];
        assert_eq!(apply_rules(&rules, "ls -la"), "ls -la");
    }

    #[test]
    fn apply_rules_empty_rules_returns_original() {
        assert_eq!(apply_rules(&[], "git status"), "git status");
    }

    #[test]
    fn apply_rules_capture_groups() {
        let rules = vec![RewriteRule {
            match_pattern: r"^(git) (status)".to_string(),
            replace: "wrapped {1} {2}".to_string(),
        }];
        assert_eq!(apply_rules(&rules, "git status"), "wrapped git status");
    }

    #[test]
    fn apply_rules_invalid_regex_skipped() {
        let rules = vec![
            RewriteRule {
                match_pattern: "[invalid".to_string(),
                replace: "bad".to_string(),
            },
            RewriteRule {
                match_pattern: r"^git status(\s.*)?$".to_string(),
                replace: "tokf run {0}".to_string(),
            },
        ];
        assert_eq!(apply_rules(&rules, "git status"), "tokf run git status");
    }

    // --- interpolate_rewrite ---

    #[test]
    fn interpolate_full_match() {
        let re = Regex::new(r"^git status(\s.*)?$").unwrap();
        let caps = re.captures("git status --short").unwrap();
        let result = interpolate_rewrite("tokf run {0}", &caps, "git status --short");
        assert_eq!(result, "tokf run git status --short");
    }

    #[test]
    fn interpolate_rest() {
        let re = Regex::new(r"^git status").unwrap();
        let caps = re.captures("git status --short -b").unwrap();
        let result =
            interpolate_rewrite("tokf run git status {rest}", &caps, "git status --short -b");
        assert_eq!(result, "tokf run git status --short -b");
    }

    #[test]
    fn interpolate_rest_empty() {
        let re = Regex::new(r"^git status$").unwrap();
        let caps = re.captures("git status").unwrap();
        let result = interpolate_rewrite("tokf run git status {rest}", &caps, "git status");
        assert_eq!(result, "tokf run git status ");
    }
}
