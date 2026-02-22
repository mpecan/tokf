use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use anyhow::Context;
use rkyv::{Archive, Deserialize, Serialize, rancor};

use super::types::FilterConfig;
use super::{ResolvedFilter, discover_all_filters};

const CACHE_VERSION: u32 = 5;

/// A single filter serialized for the binary cache.
///
/// `FilterConfig` uses `#[serde(untagged)]` on `CommandPattern`, which most
/// binary serializers cannot handle directly. We therefore serialize the
/// config as a JSON string and embed it in the binary blob.
#[derive(Debug, Clone, Archive, Serialize, Deserialize)]
pub struct CachedFilter {
    /// `FilterConfig` serialized as a JSON string.
    pub config_json: String,
    /// `source_path` stored as a UTF-8 string via `to_string_lossy()`. Non-UTF-8 path bytes
    /// are replaced with U+FFFD — filters still work correctly; only the displayed path is affected.
    pub source_path: String,
    pub relative_path: String,
    pub priority: u8,
}

/// The on-disk binary manifest: version guard, mtime fingerprints, and the filter list.
#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct ResolvedManifest {
    pub version: u32,
    /// `(dir_path_string, mtime_nanos_since_epoch)` for each search dir plus `"<binary>"`.
    pub dir_mtimes: Vec<(String, u64)>,
    pub filters: Vec<CachedFilter>,
}

fn filter_to_cached(rf: &ResolvedFilter) -> anyhow::Result<CachedFilter> {
    Ok(CachedFilter {
        config_json: serde_json::to_string(&rf.config).context("serialize FilterConfig")?,
        source_path: rf.source_path.to_string_lossy().into_owned(),
        relative_path: rf.relative_path.to_string_lossy().into_owned(),
        priority: rf.priority,
    })
}

fn cached_to_filter(cf: CachedFilter) -> anyhow::Result<ResolvedFilter> {
    Ok(ResolvedFilter {
        config: serde_json::from_str::<FilterConfig>(&cf.config_json)
            .context("deserialize FilterConfig")?,
        source_path: PathBuf::from(cf.source_path),
        relative_path: PathBuf::from(cf.relative_path),
        priority: cf.priority,
    })
}

/// Determine where to write the cache manifest.
///
/// - If `search_dirs[0]`'s parent (`.tokf/`) exists on disk → use `.tokf/cache/manifest.bin`
/// - Otherwise → use `<user_cache_dir>/tokf/manifest.bin`
/// - Returns `None` if no cache location can be determined.
pub fn cache_path(search_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(first_dir) = search_dirs.first()
        && let Some(tokf_dir) = first_dir.parent()
        && tokf_dir.exists()
    {
        return Some(tokf_dir.join("cache/manifest.bin"));
    }
    dirs::cache_dir().map(|d| d.join("tokf/manifest.bin"))
}

/// Return the mtime of `path` as nanoseconds since the Unix epoch, or 0 on error.
///
/// Nanosecond precision ensures that sub-second file writes are detected on
/// high-resolution filesystems (APFS, ext4 with `noatime`).
fn dir_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| {
            d.as_secs()
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::from(d.subsec_nanos()))
        })
}

fn binary_mtime() -> u64 {
    static CACHE: OnceLock<u64> = OnceLock::new();
    *CACHE.get_or_init(|| std::env::current_exe().ok().as_deref().map_or(0, dir_mtime))
}

fn compute_mtimes(search_dirs: &[PathBuf]) -> Vec<(String, u64)> {
    let mut mtimes: Vec<(String, u64)> = search_dirs
        .iter()
        .map(|d| (d.to_string_lossy().into_owned(), dir_mtime(d)))
        .collect();
    mtimes.push(("<binary>".to_string(), binary_mtime()));
    mtimes
}

/// Returns true iff the cached manifest is still valid for the given search dirs.
pub fn is_cache_valid(manifest: &ResolvedManifest, search_dirs: &[PathBuf]) -> bool {
    if manifest.version != CACHE_VERSION {
        return false;
    }
    manifest.dir_mtimes == compute_mtimes(search_dirs)
}

/// Load a previously written manifest from disk.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the binary data is malformed.
pub fn load_manifest(path: &Path) -> anyhow::Result<ResolvedManifest> {
    let data = std::fs::read(path).context("read cache file")?;
    rkyv::from_bytes::<ResolvedManifest, rancor::Error>(&data)
        .map_err(|e| anyhow::anyhow!("deserialize cache: {e}"))
}

fn write_manifest(
    path: &Path,
    filters: &[ResolvedFilter],
    search_dirs: &[PathBuf],
) -> anyhow::Result<()> {
    let cached: anyhow::Result<Vec<CachedFilter>> = filters.iter().map(filter_to_cached).collect();
    let manifest = ResolvedManifest {
        version: CACHE_VERSION,
        dir_mtimes: compute_mtimes(search_dirs),
        filters: cached?,
    };
    let data = rkyv::to_bytes::<rancor::Error>(&manifest)
        .map_err(|e| anyhow::anyhow!("serialize cache: {e}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cache path has no parent"))?;
    std::fs::create_dir_all(parent).context("create cache dir")?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &data).context("write cache tmp")?;
    std::fs::rename(&tmp, path).context("rename cache tmp to final")?;
    Ok(())
}

/// Discover all filters using the binary cache when possible.
///
/// Flow:
/// 1. Determine cache path; if none, fall through to `discover_all_filters`.
/// 2. Try to load and validate the cached manifest; on hit, return immediately.
/// 3. On miss: call `discover_all_filters`, attempt to persist the result, then return.
///
/// Cache write failures are logged to stderr but never propagated.
///
/// # Errors
///
/// Returns `Err` only if `discover_all_filters` itself fails (unexpected I/O error).
pub fn discover_with_cache(search_dirs: &[PathBuf]) -> anyhow::Result<Vec<ResolvedFilter>> {
    let Some(path) = cache_path(search_dirs) else {
        return discover_all_filters(search_dirs);
    };

    if let Ok(manifest) = load_manifest(&path)
        && is_cache_valid(&manifest, search_dirs)
    {
        let result: anyhow::Result<Vec<ResolvedFilter>> =
            manifest.filters.into_iter().map(cached_to_filter).collect();
        if let Ok(filters) = result {
            return Ok(filters);
        }
        // JSON deserialization failed — fall through to a full rebuild
    }

    let filters = discover_all_filters(search_dirs)?;
    if let Err(e) = write_manifest(&path, &filters, search_dirs) {
        eprintln!("[tokf] cache write failed: {e:#}");
    }
    Ok(filters)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn make_resolved_filter(command: &str, priority: u8) -> ResolvedFilter {
        let config: FilterConfig = toml::from_str(&format!("command = \"{command}\"")).unwrap();
        ResolvedFilter {
            config,
            source_path: PathBuf::from(format!("/fake/{command}.toml")),
            relative_path: PathBuf::from(format!("{command}.toml")),
            priority,
        }
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let rf = make_resolved_filter("echo test", 0);
        let cached = filter_to_cached(&rf).unwrap();
        let manifest = ResolvedManifest {
            version: CACHE_VERSION,
            dir_mtimes: vec![("<binary>".to_string(), 42)],
            filters: vec![cached],
        };
        let data = rkyv::to_bytes::<rancor::Error>(&manifest).unwrap();
        let manifest2 = rkyv::from_bytes::<ResolvedManifest, rancor::Error>(&data).unwrap();

        assert_eq!(manifest2.version, CACHE_VERSION);
        assert_eq!(manifest2.filters.len(), 1);
        assert_eq!(manifest2.dir_mtimes, vec![("<binary>".to_string(), 42u64)]);

        let rf2 = cached_to_filter(manifest2.filters.into_iter().next().unwrap()).unwrap();
        assert_eq!(rf2.config.command.first(), "echo test");
    }

    #[test]
    fn stale_on_version_mismatch() {
        let manifest = ResolvedManifest {
            version: 0, // wrong version
            dir_mtimes: compute_mtimes(&[]),
            filters: vec![],
        };
        assert!(!is_cache_valid(&manifest, &[]));
    }

    #[test]
    fn stale_on_dir_mtime_change() {
        let tmp = TempDir::new().unwrap();
        let filters_dir = tmp.path().join("filters");
        fs::create_dir_all(&filters_dir).unwrap();
        let search_dirs = vec![filters_dir.clone()];

        let manifest = ResolvedManifest {
            version: CACHE_VERSION,
            dir_mtimes: compute_mtimes(&search_dirs),
            filters: vec![],
        };
        assert!(is_cache_valid(&manifest, &search_dirs));

        // Brief pause then write a file to update the directory mtime
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(filters_dir.join("new.toml"), "command = \"new\"").unwrap();

        assert!(!is_cache_valid(&manifest, &search_dirs));
    }

    #[test]
    fn cache_path_project_local() {
        let tmp = TempDir::new().unwrap();
        let tokf_dir = tmp.path().join(".tokf");
        fs::create_dir_all(&tokf_dir).unwrap();
        let search_dirs = vec![tokf_dir.join("filters")];

        let path = cache_path(&search_dirs).unwrap();
        assert!(path.starts_with(&tokf_dir));
        assert!(path.ends_with("cache/manifest.bin"));
    }

    #[test]
    fn cache_path_user_fallback() {
        // A parent path that definitely doesn't exist on disk
        let search_dirs = vec![PathBuf::from("/tokf_test_nonexistent_dir/.tokf/filters")];
        let path = cache_path(&search_dirs);

        if let Some(user_cache) = dirs::cache_dir() {
            assert_eq!(path, Some(user_cache.join("tokf/manifest.bin")));
        } else {
            assert!(path.is_none());
        }
    }

    #[test]
    fn write_failure_does_not_propagate() {
        let tmp = TempDir::new().unwrap();
        let tokf_dir = tmp.path().join(".tokf");
        fs::create_dir_all(&tokf_dir).unwrap();
        // Block cache dir creation by placing a regular file at that path
        fs::write(tokf_dir.join("cache"), b"not a directory").unwrap();

        let search_dirs = vec![tokf_dir.join("filters")];
        let result = discover_with_cache(&search_dirs);
        assert!(result.is_ok());
    }

    #[test]
    fn cached_filter_roundtrip() {
        let config: FilterConfig = toml::from_str("command = \"git push\"").unwrap();
        let rf = ResolvedFilter {
            config,
            source_path: PathBuf::from("/some/path/push.toml"),
            relative_path: PathBuf::from("git/push.toml"),
            priority: 1,
        };
        let cached = filter_to_cached(&rf).unwrap();
        let rf2 = cached_to_filter(cached).unwrap();

        assert_eq!(rf2.config.command.first(), "git push");
        assert_eq!(rf2.source_path, PathBuf::from("/some/path/push.toml"));
        assert_eq!(rf2.relative_path, PathBuf::from("git/push.toml"));
        assert_eq!(rf2.priority, 1);
    }

    #[test]
    fn binary_sentinel_in_mtimes() {
        let mtimes = compute_mtimes(&[]);
        assert!(mtimes.iter().any(|(k, _)| k == "<binary>"));
    }

    #[test]
    fn stale_cache_triggers_rebuild() {
        let tmp = TempDir::new().unwrap();
        let tokf_dir = tmp.path().join(".tokf");
        let filters_dir = tokf_dir.join("filters");
        fs::create_dir_all(&filters_dir).unwrap();

        fs::write(filters_dir.join("first.toml"), "command = \"first cmd\"").unwrap();
        let search_dirs = vec![filters_dir.clone()];

        // First run: populates cache
        let filters1 = discover_with_cache(&search_dirs).unwrap();
        let count1 = filters1.iter().filter(|f| f.priority < u8::MAX).count();
        assert_eq!(count1, 1);

        // Brief pause then add a new filter (updates dir mtime)
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(filters_dir.join("second.toml"), "command = \"second cmd\"").unwrap();

        // Second run: cache is stale, rebuilds with both filters
        let filters2 = discover_with_cache(&search_dirs).unwrap();
        let count2 = filters2.iter().filter(|f| f.priority < u8::MAX).count();
        assert_eq!(count2, 2);
    }
}
