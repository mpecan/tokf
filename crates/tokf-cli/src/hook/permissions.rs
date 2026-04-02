//! Check commands against Claude Code's deny/ask permission rules.
//!
//! Before auto-allowing a rewritten command, we check the *original* command
//! against the user's permission rules from Claude Code `settings.json` files.
//! This prevents the hook from silently bypassing deny and ask rules.
//!
//! See: <https://github.com/rtk-ai/rtk/pull/576>

use serde_json::Value;
use std::path::PathBuf;

pub use tokf_hook_types::{PermissionDecision, PermissionVerdict};

/// Check `cmd` against Claude Code's deny/ask permission rules.
///
/// Returns `Allow` when no rules match, `Deny` when a deny rule matches,
/// or `Ask` when an ask rule matches. Deny takes priority over Ask.
pub fn check_command(cmd: &str) -> PermissionVerdict {
    let (deny_rules, ask_rules) = load_deny_ask_rules();
    check_command_with_rules(cmd, &deny_rules, &ask_rules)
}

/// Internal implementation allowing tests to inject rules without file I/O.
pub(crate) fn check_command_with_rules(
    cmd: &str,
    deny_rules: &[String],
    ask_rules: &[String],
) -> PermissionVerdict {
    let segments = split_compound_command(cmd);
    let mut any_ask = false;

    for segment in &segments {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        for pattern in deny_rules {
            if command_matches_pattern(segment, pattern) {
                return PermissionVerdict::deny(None);
            }
        }

        if !any_ask {
            for pattern in ask_rules {
                if command_matches_pattern(segment, pattern) {
                    any_ask = true;
                    break;
                }
            }
        }
    }

    if any_ask {
        PermissionVerdict::ask(None)
    } else {
        PermissionVerdict::allow()
    }
}

/// Load deny and ask Bash rules from all Claude Code settings files.
///
/// Files read (all merged):
/// 1. `$PROJECT_ROOT/.claude/settings.json`
/// 2. `$PROJECT_ROOT/.claude/settings.local.json`
/// 3. `~/.claude/settings.json`
/// 4. `~/.claude/settings.local.json`
pub(crate) fn load_deny_ask_rules() -> (Vec<String>, Vec<String>) {
    load_rules_from_paths(&get_settings_paths())
}

/// Load deny and ask Bash rules from the given settings file paths.
///
/// Missing files are silently skipped. Files that exist but contain malformed
/// JSON cause a fail-closed response: a stderr warning is emitted and a
/// wildcard `*` ask rule is injected so that the hook never silently
/// auto-allows when permissions can't be determined.
fn load_rules_from_paths<P: AsRef<std::path::Path>>(paths: &[P]) -> (Vec<String>, Vec<String>) {
    let mut deny_rules = Vec::new();
    let mut ask_rules = Vec::new();

    for path in paths {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!(
                    "[tokf] warning: could not read {}: {e} — failing closed (ask for all)",
                    path.as_ref().display()
                );
                ask_rules.push("*".to_string());
                continue;
            }
        };
        let json = match serde_json::from_str::<Value>(&content) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[tokf] warning: could not parse {}: {e} — failing closed (ask for all)",
                    path.as_ref().display()
                );
                ask_rules.push("*".to_string());
                continue;
            }
        };
        let Some(permissions) = json.get("permissions") else {
            continue;
        };

        append_bash_rules(permissions.get("deny"), &mut deny_rules);
        append_bash_rules(permissions.get("ask"), &mut ask_rules);
    }

    (deny_rules, ask_rules)
}

/// Extract `Bash(...)` patterns from a JSON array and append to `target`.
fn append_bash_rules(rules_value: Option<&Value>, target: &mut Vec<String>) {
    let Some(arr) = rules_value.and_then(|v| v.as_array()) else {
        return;
    };
    for rule in arr {
        if let Some(s) = rule.as_str()
            && let Some(pattern) = extract_bash_pattern(s)
        {
            target.push(pattern.to_string());
        }
    }
}

/// Return the ordered list of Claude Code settings file paths to check.
fn get_settings_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(root) = find_project_root() {
        paths.push(root.join(".claude").join("settings.json"));
        paths.push(root.join(".claude").join("settings.local.json"));
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".claude").join("settings.json"));
        paths.push(home.join(".claude").join("settings.local.json"));
    }

    paths
}

/// Locate the project root by walking up from CWD looking for `.claude/`.
///
/// Only checks the filesystem for `.claude/` directories — does not spawn
/// external processes (e.g. `git`), since the hook runs on every tool call
/// and the fallback would add latency without changing behavior (the derived
/// settings files still won't exist in projects without `.claude/`).
fn find_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".claude").is_dir() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }

    None
}

/// Extract the inner pattern from `Bash(pattern)`. Returns `None` for non-Bash rules.
fn extract_bash_pattern(rule: &str) -> Option<&str> {
    rule.strip_prefix("Bash(")
        .and_then(|inner| inner.strip_suffix(')'))
}

/// Check if `cmd` matches a Claude Code permission pattern.
///
/// Supports `*` as a wildcard anywhere in the pattern:
/// - `*` → matches everything
/// - `git push *` → trailing wildcard (prefix match)
/// - `* --force` → leading wildcard (suffix match)
/// - `git * main` → middle wildcard
/// - `* --help *` → multiple wildcards
/// - `sudo:*` → legacy colon syntax (prefix match)
/// - `pattern` (no wildcard) → exact match or word-boundary prefix match
fn command_matches_pattern(cmd: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        // No wildcards — exact match or prefix with word boundary.
        return starts_with_word(cmd, pattern);
    }

    // Iterate over segments between `*` wildcards. Each non-empty segment
    // must appear in order with word-boundary semantics (no partial-token
    // matches). Avoids allocating a Vec by using the split iterator directly.
    let ends_with_star = pattern.ends_with('*');
    let mut split = pattern.split('*').peekable();
    let mut pos = 0;
    let mut is_first = true;

    while let Some(segment) = split.next() {
        let is_last = split.peek().is_none();
        let seg = if is_first {
            segment.trim_end_matches(':').trim_end()
        } else {
            segment.trim()
        };

        if seg.is_empty() {
            is_first = false;
            continue;
        }

        if is_first {
            // First segment must match at the start (word boundary).
            if !starts_with_word(cmd, seg) {
                return false;
            }
            pos = seg.len();
        } else if is_last && !ends_with_star {
            // Last segment (pattern doesn't end with `*`) must match at the end
            // with a word boundary.
            return ends_with_word(cmd, seg);
        } else {
            // Middle segments: find with word-boundary awareness.
            match find_word(cmd, pos, seg) {
                Some(end) => pos = end,
                None => return false,
            }
        }

        is_first = false;
    }

    true
}

/// Check if `cmd` equals `word` or starts with `word` followed by a space.
fn starts_with_word(cmd: &str, word: &str) -> bool {
    cmd == word
        || (cmd.len() > word.len() && cmd.as_bytes()[word.len()] == b' ' && cmd.starts_with(word))
}

/// Check if `cmd` ends with `word` preceded by a space (or equals `word`).
fn ends_with_word(cmd: &str, word: &str) -> bool {
    cmd == word
        || (cmd.len() > word.len()
            && cmd.as_bytes()[cmd.len() - word.len() - 1] == b' '
            && cmd.ends_with(word))
}

/// Find `needle` in `cmd[from..]` at a word boundary: preceded by a space
/// (or at the start) and followed by a space (or at the end).
/// Returns `Some(end_pos)` (absolute index in `cmd`) on match, `None` otherwise.
fn find_word(cmd: &str, from: usize, needle: &str) -> Option<usize> {
    let haystack = &cmd[from..];
    let mut search_from = 0;
    while let Some(idx) = haystack[search_from..].find(needle) {
        let abs_start = from + search_from + idx;
        let abs_end = abs_start + needle.len();
        let left_ok = abs_start == 0 || cmd.as_bytes()[abs_start - 1] == b' ';
        let right_ok = abs_end == cmd.len() || cmd.as_bytes()[abs_end] == b' ';
        if left_ok && right_ok {
            return Some(abs_end);
        }
        search_from += idx + 1;
    }
    None
}

/// Split a compound shell command into individual segments.
///
/// Uses the tree-sitter-bash AST for quote-aware splitting on `&&`, `||`,
/// and `;`. Pipes (`|`) are intentionally excluded — `git log | head` is
/// one logical command for permission purposes.
fn split_compound_command(cmd: &str) -> Vec<String> {
    crate::rewrite::bash_ast::split_compound(cmd)
        .into_iter()
        .map(|(seg, _sep)| seg)
        .collect()
}

/// Check a command against permissions loaded from specific settings files.
///
/// Used for testing with explicit paths instead of auto-discovered ones.
#[cfg(test)]
pub(crate) fn check_command_from_settings(
    cmd: &str,
    settings_paths: &[&std::path::Path],
) -> PermissionVerdict {
    let (deny_rules, ask_rules) = load_rules_from_paths(settings_paths);
    check_command_with_rules(cmd, &deny_rules, &ask_rules)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn extract_bash_pattern_valid() {
        assert_eq!(
            extract_bash_pattern("Bash(git push --force)"),
            Some("git push --force")
        );
        assert_eq!(extract_bash_pattern("Bash(*)"), Some("*"));
        assert_eq!(extract_bash_pattern("Bash(sudo:*)"), Some("sudo:*"));
    }

    #[test]
    fn extract_bash_pattern_non_bash() {
        assert_eq!(extract_bash_pattern("Read(**/.env*)"), None);
        assert_eq!(extract_bash_pattern("Write(*)"), None);
    }

    #[test]
    fn exact_match() {
        assert!(command_matches_pattern(
            "git push --force",
            "git push --force"
        ));
    }

    #[test]
    fn prefix_match_with_args() {
        assert!(command_matches_pattern(
            "git push --force origin main",
            "git push --force"
        ));
    }

    #[test]
    fn no_partial_word_match() {
        assert!(!command_matches_pattern(
            "git push --forceful",
            "git push --force"
        ));
    }

    #[test]
    fn wildcard_all() {
        assert!(command_matches_pattern("anything at all", "*"));
        assert!(command_matches_pattern("", "*"));
    }

    #[test]
    fn wildcard_colon_prefix() {
        assert!(command_matches_pattern("sudo rm -rf /", "sudo:*"));
    }

    #[test]
    fn wildcard_colon_no_false_positive() {
        assert!(!command_matches_pattern("sudoedit /etc/hosts", "sudo:*"));
    }

    #[test]
    fn wildcard_leading() {
        assert!(command_matches_pattern("git push --force", "* --force"));
        assert!(command_matches_pattern("cargo build --force", "* --force"));
    }

    #[test]
    fn wildcard_leading_no_match() {
        assert!(!command_matches_pattern("git push --forceful", "* --force"));
    }

    #[test]
    fn wildcard_middle() {
        assert!(command_matches_pattern("git push main", "git * main"));
        assert!(command_matches_pattern("git merge main", "git * main"));
        assert!(command_matches_pattern(
            "git rebase --onto main",
            "git * main"
        ));
    }

    #[test]
    fn wildcard_middle_no_match() {
        assert!(!command_matches_pattern("git push develop", "git * main"));
    }

    #[test]
    fn wildcard_middle_no_partial_token() {
        // "git * main" must not match "xmain" — word boundary required at end.
        assert!(!command_matches_pattern("git push xmain", "git * main"));
        // Must not match "mainly" either.
        assert!(!command_matches_pattern("git push mainly", "git * main"));
    }

    #[test]
    fn wildcard_multiple() {
        assert!(command_matches_pattern(
            "git push --help origin",
            "* --help *"
        ));
        assert!(command_matches_pattern(
            "cargo test --help --verbose",
            "* --help *"
        ));
    }

    #[test]
    fn wildcard_multiple_no_partial_token() {
        // "* --help *" must not match "--helpful" — word boundary required.
        assert!(!command_matches_pattern(
            "git push --helpful origin",
            "* --help *"
        ));
    }

    #[test]
    fn wildcard_trailing_space() {
        assert!(command_matches_pattern(
            "git push --force origin main",
            "git push *"
        ));
    }

    #[test]
    fn no_match() {
        assert!(!command_matches_pattern("git status", "git push --force"));
    }

    #[test]
    fn empty_rules_allow() {
        assert_eq!(
            check_command_with_rules("git push --force", &[], &[]),
            PermissionVerdict::allow()
        );
    }

    #[test]
    fn deny_verdict() {
        let deny = vec!["git push --force".to_string()];
        assert_eq!(
            check_command_with_rules("git push --force", &deny, &[]),
            PermissionVerdict::deny(None)
        );
    }

    #[test]
    fn ask_verdict() {
        let ask = vec!["git push".to_string()];
        assert_eq!(
            check_command_with_rules("git push origin main", &[], &ask),
            PermissionVerdict::ask(None)
        );
    }

    #[test]
    fn deny_precedence_over_ask() {
        let deny = vec!["git push --force".to_string()];
        let ask = vec!["git push --force".to_string()];
        assert_eq!(
            check_command_with_rules("git push --force", &deny, &ask),
            PermissionVerdict::deny(None)
        );
    }

    #[test]
    fn compound_command_deny() {
        let deny = vec!["git push --force".to_string()];
        assert_eq!(
            check_command_with_rules("git status && git push --force", &deny, &[]),
            PermissionVerdict::deny(None)
        );
    }

    #[test]
    fn compound_command_ask() {
        let ask = vec!["git push".to_string()];
        assert_eq!(
            check_command_with_rules("git status && git push origin main", &[], &ask),
            PermissionVerdict::ask(None)
        );
    }

    #[test]
    fn compound_deny_overrides_ask() {
        let deny = vec!["git push --force".to_string()];
        let ask = vec!["git status".to_string()];
        assert_eq!(
            check_command_with_rules("git status && git push --force", &deny, &ask),
            PermissionVerdict::deny(None)
        );
    }

    #[test]
    fn malformed_settings_fails_closed_as_ask() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "not valid json {{{").unwrap();

        // Malformed JSON should inject a wildcard ask rule, causing Ask verdict.
        assert_eq!(
            check_command_from_settings("git status", &[settings.as_path()]),
            PermissionVerdict::ask(None)
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_settings_fails_closed_as_ask() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, r#"{"permissions":{"deny":["Bash(*)"]}}"#).unwrap();
        // Make the file unreadable
        std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Unreadable file should fail closed as Ask, not silently skip.
        assert_eq!(
            check_command_from_settings("git status", &[settings.as_path()]),
            PermissionVerdict::ask(None)
        );

        // Restore permissions for cleanup
        std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn missing_settings_file_allows() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = dir.path().join("nonexistent.json");

        // Missing file is silently skipped — no rules means Allow.
        assert_eq!(
            check_command_from_settings("git status", &[settings.as_path()]),
            PermissionVerdict::allow()
        );
    }

    #[test]
    fn settings_file_integration() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{
                "permissions": {
                    "deny": ["Bash(git push --force)", "Read(**/.env*)"],
                    "ask": ["Bash(git push)"]
                }
            }"#,
        )
        .unwrap();

        // Deny rule matches
        assert_eq!(
            check_command_from_settings("git push --force", &[settings.as_path()]),
            PermissionVerdict::deny(None)
        );

        // Ask rule matches
        assert_eq!(
            check_command_from_settings("git push origin main", &[settings.as_path()]),
            PermissionVerdict::ask(None)
        );

        // No rule matches
        assert_eq!(
            check_command_from_settings("git status", &[settings.as_path()]),
            PermissionVerdict::allow()
        );

        // Non-Bash rules ignored
        assert_eq!(
            check_command_from_settings("cat .env", &[settings.as_path()]),
            PermissionVerdict::allow()
        );
    }
}
