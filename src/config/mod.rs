pub mod cache;
pub mod types;

use std::path::{Path, PathBuf};

use anyhow::Context;
use include_dir::{Dir, DirEntry, include_dir};

use types::{CommandPattern, FilterConfig};

static STDLIB: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/filters");

/// Returns the embedded TOML content for a filter, if it exists.
/// `relative_path` should be like `git/push.toml`.
pub fn get_embedded_filter(relative_path: &Path) -> Option<&'static str> {
    STDLIB.get_file(relative_path)?.contents_utf8()
}

/// Build default search dirs in priority order:
/// 1. `.tokf/filters/` (repo-local, resolved from CWD)
/// 2. `{config_dir}/tokf/filters/` (user-level, platform-native)
///
/// The embedded stdlib is always appended at the end by `discover_all_filters`,
/// so no binary-adjacent path is needed.
pub fn default_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // 1. Repo-local override (resolved to absolute so it survives any later CWD change)
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join(".tokf/filters"));
    }

    // 2. User-level config dir (platform-native)
    if let Some(config) = dirs::config_dir() {
        dirs.push(config.join("tokf/filters"));
    }

    dirs
}

/// Try to load a filter from `path`. Returns `Ok(Some(config))` on success,
/// `Ok(None)` if the file does not exist, or `Err` for other I/O / parse errors.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or contains invalid TOML.
pub fn try_load_filter(path: &Path) -> anyhow::Result<Option<FilterConfig>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::Error::new(e)
                .context(format!("failed to read filter file: {}", path.display())));
        }
    };
    let config: FilterConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse filter file: {}", path.display()))?;
    Ok(Some(config))
}

/// Count non-`*` words — higher = more specific.
pub fn pattern_specificity(pattern: &str) -> usize {
    pattern.split_whitespace().filter(|w| *w != "*").count()
}

/// Extract the basename from a word that might be a path.
/// Examples: `/usr/bin/ls` -> `ls`, `./mvnw` -> `mvnw`, `git` -> `git`
fn extract_basename(word: &str) -> &str {
    // Find the last path separator (/ or \)
    word.rfind(['/', '\\']).map_or(word, |pos| &word[pos + 1..])
}

/// Returns `words_consumed` if pattern matches a prefix of `words`, else `None`.
///
/// Pattern word `*` matches any single non-empty token.
/// Trailing args beyond the pattern length are allowed (prefix semantics).
/// The first word is matched by basename, so `/usr/bin/ls` matches pattern `ls`.
pub fn pattern_matches_prefix(pattern: &str, words: &[&str]) -> Option<usize> {
    let pattern_words: Vec<&str> = pattern.split_whitespace().collect();
    if pattern_words.is_empty() || words.len() < pattern_words.len() {
        return None;
    }

    for (i, pword) in pattern_words.iter().enumerate() {
        if *pword == "*" {
            if words[i].is_empty() {
                return None;
            }
        } else {
            // For the first word, compare basenames to support path variants
            let word_to_match = if i == 0 {
                extract_basename(words[i])
            } else {
                words[i]
            };
            if word_to_match != *pword {
                return None;
            }
        }
    }

    Some(pattern_words.len())
}

/// Recursively find all `.toml` files under `dir`, sorted by relative path.
/// Skips hidden entries (names starting with `.`).
///
/// Silently returns an empty vec if the directory doesn't exist or can't be read.
pub fn discover_filter_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_filter_files(dir, &mut files);
    files.sort();
    files
}

fn collect_filter_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            collect_filter_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "toml") {
            files.push(path);
        }
    }
}

/// A discovered filter with its config, source path, and priority level.
pub struct ResolvedFilter {
    pub config: FilterConfig,
    /// Absolute path to the filter file (or `<built-in>/…` for embedded filters).
    pub source_path: PathBuf,
    /// Path relative to its source search dir (for display).
    pub relative_path: PathBuf,
    /// 0 = repo-local, 1 = user-level, `u8::MAX` = built-in.
    pub priority: u8,
}

impl ResolvedFilter {
    /// Returns `words_consumed` if any of this filter's patterns match `words`.
    pub fn matches(&self, words: &[&str]) -> Option<usize> {
        for pattern in self.config.command.patterns() {
            if let Some(consumed) = pattern_matches_prefix(pattern, words) {
                return Some(consumed);
            }
        }
        None
    }

    /// Maximum specificity across all patterns (used for sorting).
    pub fn specificity(&self) -> usize {
        self.config
            .command
            .patterns()
            .iter()
            .map(|p| pattern_specificity(p))
            .max()
            .unwrap_or(0)
    }

    /// Human-readable priority label.
    pub const fn priority_label(&self) -> &'static str {
        match self.priority {
            0 => "local",
            1 => "user",
            _ => "built-in",
        }
    }
}

/// Discover all filters across `search_dirs` plus the embedded stdlib,
/// sorted by `(priority ASC, specificity DESC)`.
///
/// Embedded stdlib entries are appended at priority `u8::MAX`,
/// so local (0) and user (1) filters always shadow built-in ones.
///
/// Deduplication: first occurrence of each command pattern (by `first()` string) wins.
///
/// # Errors
///
/// Does not return errors for missing directories or invalid TOML files — those are
/// silently skipped. Returns `Err` only on unexpected I/O failures.
pub fn discover_all_filters(search_dirs: &[PathBuf]) -> anyhow::Result<Vec<ResolvedFilter>> {
    let mut all_filters: Vec<ResolvedFilter> = Vec::new();

    for (priority, dir) in search_dirs.iter().enumerate() {
        let files = discover_filter_files(dir);

        for path in files {
            let Ok(Some(config)) = try_load_filter(&path) else {
                continue;
            };

            let relative_path = path.strip_prefix(dir).unwrap_or(&path).to_path_buf();

            all_filters.push(ResolvedFilter {
                config,
                source_path: path,
                relative_path,
                priority: u8::try_from(priority).unwrap_or(u8::MAX),
            });
        }
    }

    // Append embedded stdlib at the lowest priority (u8::MAX ensures it always
    // sorts after local/user dirs regardless of how many dirs are in the slice).
    let stdlib_priority = u8::MAX;
    if let Ok(entries) = STDLIB.find("**/*.toml") {
        for entry in entries {
            if let DirEntry::File(file) = entry {
                let content = file.contents_utf8().unwrap_or("");
                let Ok(config) = toml::from_str::<FilterConfig>(content) else {
                    continue; // silently skip invalid embedded TOML
                };
                let rel = file.path().to_path_buf();
                all_filters.push(ResolvedFilter {
                    config,
                    source_path: PathBuf::from("<built-in>").join(&rel),
                    relative_path: rel,
                    priority: stdlib_priority,
                });
            }
        }
    }

    // Sort by (priority ASC, specificity DESC): lower priority number and higher
    // specificity win.
    all_filters.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| b.specificity().cmp(&a.specificity()))
    });

    // Dedup: keep first occurrence of each canonical command pattern.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    all_filters.retain(|f| seen.insert(f.config.command.first().to_string()));

    Ok(all_filters)
}

/// Build a rewrite regex pattern for a command pattern string.
/// `*` is replaced with `\S+` to match any single non-whitespace token.
pub fn command_pattern_to_regex(pattern: &str) -> String {
    let escaped_words: Vec<String> = pattern
        .split_whitespace()
        .map(|w| {
            if w == "*" {
                r"\S+".to_string()
            } else {
                regex::escape(w)
            }
        })
        .collect();
    format!("^{}(\\s.*)?$", escaped_words.join(r"\ "))
}

/// Extract command patterns as rewrite regex strings for a `CommandPattern`.
pub fn command_pattern_regexes(command: &CommandPattern) -> Vec<(String, String)> {
    command
        .patterns()
        .iter()
        .map(|p| (p.clone(), command_pattern_to_regex(p)))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    // --- pattern_specificity ---

    #[test]
    fn specificity_two_literals() {
        assert_eq!(pattern_specificity("git push"), 2);
    }

    #[test]
    fn specificity_wildcard_counts_less() {
        assert_eq!(pattern_specificity("git *"), 1);
        assert_eq!(pattern_specificity("* push"), 1);
    }

    #[test]
    fn specificity_all_wildcards() {
        assert_eq!(pattern_specificity("* *"), 0);
    }

    #[test]
    fn specificity_ordering() {
        // "git push" more specific than "git *" more specific than "* push"
        assert!(pattern_specificity("git push") > pattern_specificity("git *"));
        assert!(pattern_specificity("git *") == pattern_specificity("* push"));
    }

    // --- pattern_matches_prefix ---

    #[test]
    fn matches_exact() {
        let words = ["git", "push"];
        assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
    }

    #[test]
    fn matches_prefix_with_trailing_args() {
        let words = ["git", "push", "origin", "main"];
        assert_eq!(pattern_matches_prefix("git push", &words), Some(2));
    }

    #[test]
    fn matches_wildcard() {
        let words = ["npm", "run", "build"];
        assert_eq!(pattern_matches_prefix("npm run *", &words), Some(3));
    }

    #[test]
    fn no_match_different_command() {
        let words = ["cargo", "test"];
        assert_eq!(pattern_matches_prefix("git push", &words), None);
    }

    #[test]
    fn no_match_too_short() {
        let words = ["git"];
        assert_eq!(pattern_matches_prefix("git push", &words), None);
    }

    #[test]
    fn empty_pattern_returns_none() {
        let words = ["git", "push"];
        assert_eq!(pattern_matches_prefix("", &words), None);
    }

    #[test]
    fn empty_words_returns_none() {
        assert_eq!(pattern_matches_prefix("git push", &[]), None);
    }

    #[test]
    fn single_word_pattern_prefix_match() {
        assert_eq!(pattern_matches_prefix("echo", &["echo"]), Some(1));
        assert_eq!(pattern_matches_prefix("echo", &["echo", "hello"]), Some(1));
        assert_eq!(pattern_matches_prefix("echo", &["ls"]), None);
    }

    #[test]
    fn wildcard_rejects_empty_token() {
        // An empty string slice element is not a valid word match for `*`
        assert_eq!(pattern_matches_prefix("git *", &["git", ""]), None);
    }

    #[test]
    fn wildcard_at_start() {
        let words = ["my-tool", "subcommand"];
        assert_eq!(pattern_matches_prefix("* subcommand", &words), Some(2));
    }

    #[test]
    fn hyphenated_tool_not_ambiguous() {
        // golangci-lint run should match "golangci-lint run" but not "golangci-lint"
        let words = ["golangci-lint", "run"];
        assert_eq!(pattern_matches_prefix("golangci-lint run", &words), Some(2));
        assert_eq!(pattern_matches_prefix("golangci-lint", &words), Some(1));
    }

    #[test]
    fn basename_matching() {
        assert_eq!(pattern_matches_prefix("ls", &["/usr/bin/ls"]), Some(1));
        assert_eq!(
            pattern_matches_prefix("ls -la", &["/usr/bin/ls", "-la"]),
            Some(2)
        );
        assert_eq!(pattern_matches_prefix("mvnw", &["./mvnw"]), Some(1));
        assert_eq!(
            pattern_matches_prefix("git push", &["git", "push"]),
            Some(2)
        );
        assert_eq!(pattern_matches_prefix("git /p", &["git", "/p"]), Some(2));
        assert_eq!(pattern_matches_prefix("git f", &["git", "/p"]), None);
    }

    // --- discover_filter_files ---

    #[test]
    fn discover_flat_dir() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.toml"), "").unwrap();
        fs::write(dir.path().join("b.toml"), "").unwrap();
        fs::write(dir.path().join("not-toml.txt"), "").unwrap();

        let files = discover_filter_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files[0].ends_with("a.toml"));
        assert!(files[1].ends_with("b.toml"));
    }

    #[test]
    fn discover_nested_dirs() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("git");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("push.toml"), "").unwrap();
        fs::write(sub.join("status.toml"), "").unwrap();
        fs::write(dir.path().join("root.toml"), "").unwrap();

        let files = discover_filter_files(dir.path());
        assert_eq!(files.len(), 3);
        // sorted by path: git/push.toml, git/status.toml, root.toml
        assert!(files[0].ends_with("git/push.toml"));
        assert!(files[1].ends_with("git/status.toml"));
        assert!(files[2].ends_with("root.toml"));
    }

    #[test]
    fn discover_skips_hidden_entries() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".hidden.toml"), "").unwrap();
        fs::write(dir.path().join("visible.toml"), "").unwrap();
        let hidden_dir = dir.path().join(".hiddendir");
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("inside.toml"), "").unwrap();

        let files = discover_filter_files(dir.path());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("visible.toml"));
    }

    #[test]
    fn discover_nonexistent_dir_returns_empty() {
        let files = discover_filter_files(Path::new("/no/such/directory/ever"));
        assert!(files.is_empty());
    }

    // --- discover_all_filters ---

    #[test]
    fn discover_all_priority_ordering() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        // dir1 = priority 0 (local), dir2 = priority 1 (user)
        fs::write(
            dir1.path().join("my-cmd.toml"),
            "command = \"my cmd local\"",
        )
        .unwrap();
        fs::write(dir2.path().join("my-cmd.toml"), "command = \"my cmd user\"").unwrap();

        let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let filters = discover_all_filters(&dirs).unwrap();

        // Should have both (different command strings) plus embedded stdlib
        assert!(filters.len() >= 2);
        assert_eq!(filters[0].config.command.first(), "my cmd local");
        assert_eq!(filters[0].priority, 0);
    }

    #[test]
    fn discover_all_dedup_same_command() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        fs::write(dir1.path().join("a.toml"), "command = \"git push\"").unwrap();
        fs::write(dir2.path().join("b.toml"), "command = \"git push\"").unwrap();

        let dirs = vec![dir1.path().to_path_buf(), dir2.path().to_path_buf()];
        let filters = discover_all_filters(&dirs).unwrap();

        // Dedup by first() — only one entry for "git push"
        let push_entries: Vec<_> = filters
            .iter()
            .filter(|f| f.config.command.first() == "git push")
            .collect();
        assert_eq!(push_entries.len(), 1);
        assert_eq!(push_entries[0].priority, 0);
    }

    #[test]
    fn discover_all_specificity_ordering() {
        let dir = TempDir::new().unwrap();

        // More specific patterns should sort first within same priority
        fs::write(dir.path().join("a.toml"), "command = \"git *\"").unwrap();
        fs::write(dir.path().join("b.toml"), "command = \"git push\"").unwrap();

        let dirs = vec![dir.path().to_path_buf()];
        let filters = discover_all_filters(&dirs).unwrap();

        // "git push" (specificity=2) should come before "git *" (specificity=1)
        assert_eq!(filters[0].config.command.first(), "git push");
        assert_eq!(filters[1].config.command.first(), "git *");
    }

    #[test]
    fn discover_all_skips_invalid_toml() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("bad.toml"), "not valid [[[").unwrap();
        fs::write(dir.path().join("good.toml"), "command = \"my tool\"").unwrap();

        let filters = discover_all_filters(&[dir.path().to_path_buf()]).unwrap();
        let my_tool: Vec<_> = filters
            .iter()
            .filter(|f| f.config.command.first() == "my tool")
            .collect();
        assert_eq!(my_tool.len(), 1);
    }

    #[test]
    fn discover_all_hyphenated_tool_not_ambiguous() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("golangci-lint.toml"),
            "command = \"golangci-lint run\"",
        )
        .unwrap();

        let filters = discover_all_filters(&[dir.path().to_path_buf()]).unwrap();
        let golangci: Vec<_> = filters
            .iter()
            .filter(|f| f.config.command.first() == "golangci-lint run")
            .collect();
        assert_eq!(golangci.len(), 1);
        let words = ["golangci-lint", "run"];
        assert_eq!(golangci[0].matches(&words), Some(2));

        let words_no_match = ["golangci", "lint", "run"];
        assert_eq!(golangci[0].matches(&words_no_match), None);
    }

    // --- embedded stdlib tests ---

    #[test]
    fn embedded_stdlib_non_empty() {
        let entries: Vec<_> = STDLIB.find("**/*.toml").unwrap().collect();
        assert!(
            entries.len() >= 10,
            "expected at least 10 embedded filters, got {}",
            entries.len()
        );
    }

    #[test]
    fn all_embedded_toml_parse() {
        for entry in STDLIB.find("**/*.toml").unwrap() {
            if let DirEntry::File(file) = entry {
                let content = file.contents_utf8().unwrap_or("");
                assert!(
                    toml::from_str::<FilterConfig>(content).is_ok(),
                    "failed to parse embedded filter: {}",
                    file.path().display()
                );
            }
        }
    }

    #[test]
    fn embedded_filters_in_discover_with_no_dirs() {
        // With empty search dirs, only embedded stdlib is returned
        let filters = discover_all_filters(&[]).unwrap();
        assert!(
            !filters.is_empty(),
            "expected embedded stdlib filters with no search dirs"
        );
        let has_git_push = filters
            .iter()
            .any(|f| f.config.command.first() == "git push");
        assert!(has_git_push, "expected git push in embedded stdlib");
    }

    #[test]
    fn local_filter_shadows_embedded() {
        let dir = TempDir::new().unwrap();
        // Override git push locally
        fs::write(
            dir.path().join("push.toml"),
            "command = \"git push\"\n# local override",
        )
        .unwrap();

        let dirs = vec![dir.path().to_path_buf()];
        let filters = discover_all_filters(&dirs).unwrap();

        // "git push" should appear exactly once (local shadows embedded)
        let push_entries: Vec<_> = filters
            .iter()
            .filter(|f| f.config.command.first() == "git push")
            .collect();
        assert_eq!(push_entries.len(), 1);
        assert_eq!(push_entries[0].priority, 0); // local priority
    }

    // --- try_load_filter ---

    #[test]
    fn test_load_valid_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.toml");
        fs::write(&path, "command = \"echo hello\"").unwrap();

        let config = try_load_filter(&path).unwrap().unwrap();
        assert_eq!(config.command.first(), "echo hello");
    }

    #[test]
    fn test_load_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "not valid toml [[[").unwrap();

        assert!(try_load_filter(&path).is_err());
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let path = PathBuf::from("/tmp/nonexistent-tokf-test-file.toml");
        assert!(try_load_filter(&path).unwrap().is_none());
    }

    #[test]
    fn test_load_real_stdlib_filter() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("filters/git/push.toml");
        let config = try_load_filter(&path).unwrap().unwrap();
        assert_eq!(config.command.first(), "git push");
    }

    // --- default_search_dirs ---

    #[test]
    fn test_default_search_dirs_non_empty_and_starts_with_local() {
        let dirs = default_search_dirs();
        assert!(!dirs.is_empty());
        assert!(
            dirs[0].is_absolute(),
            "first dir should be absolute, got: {:?}",
            dirs[0]
        );
        assert!(
            dirs[0].ends_with(".tokf/filters"),
            "first dir should end with .tokf/filters, got: {:?}",
            dirs[0]
        );
    }

    #[test]
    fn test_default_search_dirs_only_local_and_user() {
        let dirs = default_search_dirs();
        // Should have at most 2 dirs: local (.tokf/filters) and user config
        // The binary-adjacent path has been removed; embedded stdlib replaces it.
        assert!(
            dirs.len() <= 2,
            "expected at most 2 search dirs (local + user), got {}: {:?}",
            dirs.len(),
            dirs
        );
    }

    // --- command_pattern_to_regex ---

    #[test]
    fn regex_from_literal_pattern() {
        let r = command_pattern_to_regex("git push");
        let re = regex::Regex::new(&r).unwrap();
        assert!(re.is_match("git push"));
        assert!(re.is_match("git push origin main"));
        assert!(!re.is_match("git status"));
    }

    #[test]
    fn regex_from_wildcard_pattern() {
        let r = command_pattern_to_regex("npm run *");
        let re = regex::Regex::new(&r).unwrap();
        assert!(re.is_match("npm run build"));
        assert!(re.is_match("npm run test --watch"));
        assert!(!re.is_match("npm run"));
        assert!(!re.is_match("npm install"));
    }
}
