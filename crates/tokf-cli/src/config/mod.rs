pub mod cache;
pub mod types;
pub mod variant;

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

/// Returns all embedded files under `dir_path` as `(relative_path, utf8_content)` pairs.
/// `dir_path` is relative to the stdlib root (e.g. `"cargo/build_test"`).
pub fn get_embedded_dir_files(dir_path: &Path) -> Vec<(PathBuf, &'static str)> {
    let Some(dir) = STDLIB.get_dir(dir_path) else {
        return Vec::new();
    };
    dir.files()
        .filter_map(|f| Some((f.path().to_path_buf(), f.contents_utf8()?)))
        .collect()
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

/// Skip flag-like tokens at the start of `words` until `target` is found.
///
/// Used to transparently handle global flags between a command name and its
/// subcommand, e.g. `git -C /path log` where `-C /path` are skipped when
/// matching against the pattern `git log`.
///
/// Returns the number of elements consumed from `words` **including** `target`,
/// or `None` if `target` was not found after only flag-like tokens.
///
/// Skipping rules:
/// - `--flag=value` : single token
/// - `-f` / `--flag` followed by a non-flag, non-target word : two tokens
/// - `-f` / `--flag` immediately before another flag or `target` : single token
///
/// # Ambiguity note
///
/// When a flag's prospective value word equals `target`, the value is **not**
/// consumed — the target is matched at that position instead.  This means
/// `git -C log log` matches `git log` with `words_consumed = 3`, treating the
/// first `log` as `-C`'s value… but that is the correct interpretation: git
/// changes to the directory named `log` and then runs `git log`.
fn skip_flags_to_match(words: &[&str], target: &str) -> Option<usize> {
    let mut i = 0;
    while i < words.len() {
        if words[i] == target {
            return Some(i + 1);
        }
        if words[i].starts_with('-') {
            if words[i].contains('=') {
                // --flag=value: entire flag is a single token
                i += 1;
            } else {
                // -f or --flag: skip the flag itself
                i += 1;
                // If the next word is not a flag and not our target, treat it
                // as the flag's value argument and skip it too.
                if i < words.len() && !words[i].starts_with('-') && words[i] != target {
                    i += 1;
                }
            }
        } else {
            // Non-flag, non-target: cannot skip transparently
            return None;
        }
    }
    None
}

/// Returns `words_consumed` if pattern matches a prefix of `words`, else `None`.
///
/// Pattern word `*` matches any single non-empty token.
/// Trailing args beyond the pattern length are allowed (prefix semantics).
/// The first word is matched by basename, so `/usr/bin/git` matches pattern `git`.
///
/// Between consecutive pattern words, flag-like tokens (`-f`, `--flag`,
/// `--flag=value`) are skipped transparently, so `git -C /path log` matches
/// pattern `git log`.  The returned count includes any transparently-skipped
/// tokens, ensuring `command_args[..consumed]` still forms the full command
/// prefix (with the global flags in-place) when the command is re-executed.
///
/// # Implementation note
///
/// `word_idx` (the position in `words`) is tracked independently of the
/// pattern index so that transparently-skipped flag tokens advance `word_idx`
/// without advancing the pattern position.  As a result, the returned count
/// may exceed `pattern.split_whitespace().count()`.  This is intentional and
/// correct: the caller uses `command_args[..consumed]` as the full command
/// prefix, which must include the global flags.
pub fn pattern_matches_prefix(pattern: &str, words: &[&str]) -> Option<usize> {
    let pattern_words: Vec<&str> = pattern.split_whitespace().collect();
    if pattern_words.is_empty() || words.is_empty() {
        return None;
    }

    // word_idx tracks our position in `words`; it advances past both matched
    // pattern tokens and any transparently-skipped flag tokens.
    let mut word_idx = 0;

    for (pat_idx, pword) in pattern_words.iter().enumerate() {
        if word_idx >= words.len() {
            return None;
        }

        if *pword == "*" {
            if words[word_idx].is_empty() {
                return None;
            }
            word_idx += 1;
        } else {
            // For the first word compare basenames, supporting path variants.
            let word_to_match = if pat_idx == 0 {
                extract_basename(words[word_idx])
            } else {
                words[word_idx]
            };

            if word_to_match == *pword {
                word_idx += 1;
            } else if pat_idx > 0 {
                // Between pattern words, try to skip over global flag tokens.
                if let Some(advance) = skip_flags_to_match(&words[word_idx..], pword) {
                    word_idx += advance;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    }

    Some(word_idx)
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
///
/// The generated regex mirrors the two runtime matching behaviours:
///
/// 1. **Basename matching** — the first word allows an optional leading path
///    prefix (`/usr/bin/`, `./`, …), so `/usr/bin/git push` matches the
///    pattern `git push`.
///
/// 2. **Transparent global flags** — between consecutive literal pattern words,
///    flag-like tokens (`-f`, `--flag`, `--flag=value`, `-f value`) are
///    tolerated, mirroring the logic in `skip_flags_to_match`.  The optional
///    flag-value group `(?:\s+[^-\s]\S*)?` naturally avoids consuming the next
///    pattern word because the NFA engine backtracks the optional group when
///    doing so is the only way the overall regex can match.  Wildcards (`*`)
///    use plain `\s+` between words.
///
/// # Examples
///
/// Pattern `"git log"` produces a regex that matches all of:
/// - `git log`
/// - `git log --oneline`
/// - `git -C /path log`
/// - `/usr/bin/git --no-pager -C /repo log --oneline`
pub fn command_pattern_to_regex(pattern: &str) -> String {
    let words: Vec<&str> = pattern.split_whitespace().collect();
    if words.is_empty() {
        return "^(\\s.*)?$".to_string();
    }

    let mut regex = String::from("^");

    for (i, &word) in words.iter().enumerate() {
        let word_re = if word == "*" {
            r"\S+".to_string()
        } else {
            regex::escape(word)
        };

        if i == 0 {
            if word == "*" {
                regex.push_str(r"\S+");
            } else {
                // Allow an optional leading path prefix (e.g. `/usr/bin/` or
                // `./`) so that `/usr/bin/git` matches the pattern `git`.
                regex.push_str(r"(?:[^\s]*/)?");
                regex.push_str(&word_re);
            }
        } else if word == "*" {
            // Wildcard: require exactly one whitespace-separated token.
            regex.push_str(r"\s+\S+");
        } else {
            // Between consecutive literal words, allow any number of flag-like
            // tokens to be skipped transparently.
            //
            // A flag segment is one of:
            //   -flag=value          single token with embedded value
            //   -flag                standalone flag (no value)
            //   -flag <value>        flag then a separate non-flag, non-target word
            //
            // The optional `(?:\s+[^-\s]\S*)?` captures a flag's value
            // argument.  When the value would consume the target pattern word,
            // the NFA engine backtracks that optional group (making it empty)
            // so that `\s+{word_re}` can match instead.
            regex.push_str(r"(?:\s+-[^=\s]+(?:=[^\s]+)?(?:\s+[^-\s]\S*)?)*\s+");
            regex.push_str(&word_re);
        }
    }

    regex.push_str(r"(\s.*)?$");
    regex
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
mod tests;
