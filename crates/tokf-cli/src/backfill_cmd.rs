use std::collections::BTreeMap;
use std::process::Command;

use serde::Serialize;
use tokf::remote::http::Client;
use tokf_common::hash::canonical_hash;

/// Entry point for the `tokf backfill-versions` subcommand.
pub fn cmd_backfill_versions(registry_url: &str, token: &str, dry_run: bool) -> i32 {
    match backfill_versions(registry_url, token, dry_run) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

// ── Request types (mirrors server) ──────────────────────────────────────────

#[derive(Debug, Serialize)]
struct BackfillVersionsRequest {
    entries: Vec<BackfillEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct BackfillEntry {
    content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    introduced_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deprecated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    successor_hash: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct BackfillResponse {
    updated: usize,
    skipped: usize,
}

// ── Core logic ──────────────────────────────────────────────────────────────

fn backfill_versions(registry_url: &str, token: &str, dry_run: bool) -> anyhow::Result<i32> {
    let tags = list_release_tags()?;
    if tags.is_empty() {
        eprintln!("[backfill] No release tags found.");
        return Ok(0);
    }
    eprintln!("[backfill] Found {} release tags", tags.len());

    // Build per-tag filter snapshots.
    let mut tag_snapshots: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for tag in &tags {
        let filters = list_filters_at_tag(tag)?;
        eprintln!("[backfill]   {tag}: {} filters", filters.len());
        tag_snapshots.push((tag.clone(), filters));
    }

    // Compute per-hash version info from the timeline.
    let entries = compute_version_timeline(&tag_snapshots);
    eprintln!(
        "[backfill] Computed version info for {} hashes",
        entries.len()
    );

    if dry_run {
        let payload = serde_json::to_string_pretty(&BackfillVersionsRequest {
            entries: entries.clone(),
        })?;
        println!("{payload}");
        eprintln!("[backfill] Dry run — payload printed above.");
        return Ok(0);
    }

    let client = Client::new(registry_url, Some(token))?;
    let resp = client.post::<_, BackfillResponse>(
        "/api/filters/backfill-versions",
        &BackfillVersionsRequest { entries },
    )?;
    eprintln!(
        "[backfill] Updated: {}, Skipped: {}",
        resp.updated, resp.skipped
    );

    Ok(0)
}

/// List all `tokf-v*` tags sorted by semver.
fn list_release_tags() -> anyhow::Result<Vec<String>> {
    let output = Command::new("git")
        .args(["tag", "-l", "tokf-v*"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git tag failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let text = String::from_utf8(output.stdout)?;
    let mut tags: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();

    // Sort by semver (strip "tokf-v" prefix).
    tags.sort_by(|a, b| {
        let va = a.strip_prefix("tokf-v").unwrap_or(a);
        let vb = b.strip_prefix("tokf-v").unwrap_or(b);
        compare_semver(va, vb)
    });

    Ok(tags)
}

/// Simple semver comparison (major.minor.patch).
fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> (u64, u64, u64) {
        let mut parts = s.split('.');
        let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };
    parse(a).cmp(&parse(b))
}

/// List filter TOML files at a given git tag and compute their content hashes.
///
/// Returns `Vec<(content_hash, command_pattern)>`.
fn list_filters_at_tag(tag: &str) -> anyhow::Result<Vec<(String, String)>> {
    // Try workspace path first; fall back to pre-workspace path when empty.
    // `git ls-tree` returns exit 0 with empty output for non-existent paths,
    // so we check `is_empty()` rather than relying on `or_else`.
    let mut paths = list_filter_paths_at_tag(tag, "crates/tokf-cli/filters/")?;
    if paths.is_empty() {
        paths = list_filter_paths_at_tag(tag, "filters/")?;
    }

    let mut results = Vec::new();
    for path in &paths {
        if let Ok(content) = git_show(tag, path) {
            if let Ok(config) = toml::from_str::<tokf_common::config::types::FilterConfig>(&content)
            {
                if let Ok(hash) = canonical_hash(&config) {
                    let cmd_pattern = config.command.first().to_string();
                    results.push((hash, cmd_pattern));
                }
            }
        }
    }
    Ok(results)
}

fn list_filter_paths_at_tag(tag: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-tree", "-r", "--name-only", tag, "--", prefix])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git ls-tree failed for {tag}:{prefix}");
    }
    let text = String::from_utf8(output.stdout)?;
    Ok(text
        .lines()
        .filter(|l| l.ends_with(".toml") && !l.contains("_test/"))
        .map(String::from)
        .collect())
}

fn git_show(tag: &str, path: &str) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["show", &format!("{tag}:{path}")])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git show {tag}:{path} failed");
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Compute version timeline from tag snapshots.
///
/// For each unique content_hash, determines:
/// - `introduced_at`: first tag where this hash appears
/// - `deprecated_at`: first tag where this hash disappears (and a successor exists)
/// - `successor_hash`: the hash with the same command_pattern at `deprecated_at`
fn compute_version_timeline(snapshots: &[(String, Vec<(String, String)>)]) -> Vec<BackfillEntry> {
    // Track: hash -> (first_seen_version, command_pattern)
    let mut first_seen: BTreeMap<String, (String, String)> = BTreeMap::new();
    // Track: hash -> (deprecated_version, successor_hash)
    let mut deprecated: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();

    let mut prev_hashes: BTreeMap<String, String> = BTreeMap::new(); // hash -> cmd_pattern

    for (tag, filters) in snapshots {
        let version = tag.strip_prefix("tokf-v").unwrap_or(tag).to_string();
        let current_hashes: BTreeMap<String, String> = filters.iter().cloned().collect();

        // Newly appeared hashes.
        for (hash, cmd) in &current_hashes {
            first_seen
                .entry(hash.clone())
                .or_insert_with(|| (version.clone(), cmd.clone()));
        }

        // Disappeared hashes (was in prev, not in current).
        if !prev_hashes.is_empty() {
            for (hash, cmd) in &prev_hashes {
                if !current_hashes.contains_key(hash) && !deprecated.contains_key(hash) {
                    // Find successor: a hash in current with the same command_pattern.
                    let successor = current_hashes
                        .iter()
                        .find(|(_, c)| *c == cmd)
                        .map(|(h, _)| h.clone());
                    deprecated.insert(hash.clone(), (version.clone(), successor));
                }
            }
        }

        prev_hashes = current_hashes;
    }

    // Build entries for all hashes we've ever seen.
    first_seen
        .into_iter()
        .map(|(hash, (intro_ver, _cmd))| {
            let (dep_ver, successor) = deprecated.remove(&hash).unwrap_or((String::new(), None));
            BackfillEntry {
                content_hash: hash,
                introduced_at: Some(intro_ver),
                deprecated_at: if dep_ver.is_empty() {
                    None
                } else {
                    Some(dep_ver)
                },
                successor_hash: successor,
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn compare_semver_ordering() {
        assert_eq!(compare_semver("0.1.0", "0.2.0"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("0.2.0", "0.2.0"), std::cmp::Ordering::Equal);
        assert_eq!(
            compare_semver("1.0.0", "0.9.9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compute_timeline_simple() {
        let snapshots = vec![
            (
                "tokf-v0.1.0".to_string(),
                vec![("hash_a".to_string(), "git push".to_string())],
            ),
            (
                "tokf-v0.2.0".to_string(),
                vec![("hash_b".to_string(), "git push".to_string())],
            ),
        ];
        let entries = compute_version_timeline(&snapshots);

        let a = entries.iter().find(|e| e.content_hash == "hash_a").unwrap();
        assert_eq!(a.introduced_at.as_deref(), Some("0.1.0"));
        assert_eq!(a.deprecated_at.as_deref(), Some("0.2.0"));
        assert_eq!(a.successor_hash.as_deref(), Some("hash_b"));

        let b = entries.iter().find(|e| e.content_hash == "hash_b").unwrap();
        assert_eq!(b.introduced_at.as_deref(), Some("0.2.0"));
        assert!(b.deprecated_at.is_none());
        assert!(b.successor_hash.is_none());
    }

    #[test]
    fn compute_timeline_stable_filter() {
        let snapshots = vec![
            (
                "tokf-v0.1.0".to_string(),
                vec![("hash_a".to_string(), "git push".to_string())],
            ),
            (
                "tokf-v0.2.0".to_string(),
                vec![("hash_a".to_string(), "git push".to_string())],
            ),
        ];
        let entries = compute_version_timeline(&snapshots);
        assert_eq!(entries.len(), 1);
        let a = &entries[0];
        assert_eq!(a.introduced_at.as_deref(), Some("0.1.0"));
        assert!(a.deprecated_at.is_none());
    }

    #[test]
    fn compute_timeline_removed_no_successor() {
        let snapshots = vec![
            (
                "tokf-v0.1.0".to_string(),
                vec![("hash_a".to_string(), "git push".to_string())],
            ),
            ("tokf-v0.2.0".to_string(), vec![]),
        ];
        let entries = compute_version_timeline(&snapshots);
        let a = &entries[0];
        assert_eq!(a.deprecated_at.as_deref(), Some("0.2.0"));
        assert!(a.successor_hash.is_none());
    }
}
