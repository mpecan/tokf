use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row as _};

use crate::storage::StorageClient;

// ── Catalog types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct CatalogAuthor {
    pub username: String,
    pub avatar_url: String,
    pub profile_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct CatalogFilterStats {
    #[cfg_attr(test, ts(type = "number"))]
    pub total_commands: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub total_input_tokens: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub total_output_tokens: i64,
    pub savings_pct: f64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub total_raw_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct CatalogEntry {
    pub content_hash: String,
    pub command_pattern: String,
    pub canonical_command: String,
    pub author: CatalogAuthor,
    pub is_stdlib: bool,
    pub created_at: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub test_count: i64,
    pub safety_passed: bool,
    pub stats: CatalogFilterStats,
    /// The tokf release version that published this filter (e.g. "0.2.28").
    /// `None` for community filters and stdlib filters published before versioning was introduced.
    pub stdlib_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct GlobalStats {
    #[cfg_attr(test, ts(type = "number"))]
    pub total_filters: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub total_commands: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub total_input_tokens: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub total_output_tokens: i64,
    pub overall_savings_pct: f64,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub total_raw_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct CatalogIndex {
    pub generated_at: String,
    pub version: u32,
    pub filters: Vec<CatalogEntry>,
    pub global_stats: GlobalStats,
}

/// Alias for the per-filter metadata written to R2 as `filters/{hash}/metadata.json`.
///
/// Structurally identical to [`CatalogEntry`] — exists as a semantic alias for
/// TypeScript consumers (tokf-net imports `FilterMetadata` for per-filter pages).
pub type FilterMetadata = CatalogEntry;

// ── R2 key helpers ───────────────────────────────────────────────────────────

/// R2 key for the full catalog index.
pub const fn catalog_index_key() -> &'static str {
    "catalog/index.json"
}

/// R2 key for a single filter's metadata.
pub fn filter_metadata_key(hash: &str) -> String {
    format!("filters/{hash}/metadata.json")
}

/// R2 key for a filter's before/after examples.
///
/// Delegates to [`crate::storage::filter_examples_key`] — single source of truth.
pub fn filter_examples_key(hash: &str) -> String {
    crate::storage::filter_examples_key(hash)
}

// ── DB queries ───────────────────────────────────────────────────────────────

/// Build the full catalog index from the database.
///
/// Fetches all filters joined with users and stats, computes global aggregates,
/// and respects the `visible` flag on users (non-visible authors are redacted).
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn build_catalog_index(pool: &PgPool) -> Result<CatalogIndex, crate::error::AppError> {
    let entries = fetch_all_entries(pool).await?;
    let global_stats = fetch_global_stats(pool).await?;

    Ok(CatalogIndex {
        generated_at: chrono::Utc::now().to_rfc3339(),
        version: 1,
        filters: entries,
        global_stats,
    })
}

/// Build metadata for a single filter by content hash.
///
/// # Errors
///
/// Returns `NotFound` if the filter doesn't exist, or an internal error on DB failure.
pub async fn build_filter_metadata(
    pool: &PgPool,
    hash: &str,
) -> Result<CatalogEntry, crate::error::AppError> {
    let row = sqlx::query(
        "SELECT f.content_hash, f.command_pattern, f.canonical_command, f.is_stdlib,
                f.created_at::TEXT AS created_at,
                f.safety_passed,
                f.stdlib_version,
                CASE WHEN u.visible THEN u.username ELSE 'tokf' END AS username,
                CASE WHEN u.visible THEN COALESCE(u.avatar_url, '') ELSE '' END AS avatar_url,
                CASE WHEN u.visible THEN COALESCE(u.profile_url, '') ELSE '' END AS profile_url,
                COALESCE(fs.total_commands, 0) AS total_commands,
                COALESCE(fs.total_input_tokens, 0) AS total_input_tokens,
                COALESCE(fs.total_output_tokens, 0) AS total_output_tokens,
                COALESCE(fs.savings_pct, 0.0) AS savings_pct,
                COALESCE(fs.total_raw_tokens, 0) AS total_raw_tokens,
                (SELECT COUNT(*)::BIGINT FROM filter_tests
                 WHERE filter_hash = f.content_hash) AS test_count
         FROM filters f
         JOIN users u ON u.id = f.author_id
         LEFT JOIN filter_stats fs ON fs.filter_hash = f.content_hash
         WHERE f.content_hash = $1",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| crate::error::AppError::NotFound(format!("filter not found: {hash}")))?;

    map_entry_row(&row).map_err(|e| crate::error::AppError::Internal(format!("db mapping: {e}")))
}

async fn fetch_all_entries(pool: &PgPool) -> Result<Vec<CatalogEntry>, crate::error::AppError> {
    let rows = sqlx::query(
        "SELECT f.content_hash, f.command_pattern, f.canonical_command, f.is_stdlib,
                f.created_at::TEXT AS created_at,
                f.safety_passed,
                f.stdlib_version,
                CASE WHEN u.visible THEN u.username ELSE 'tokf' END AS username,
                CASE WHEN u.visible THEN COALESCE(u.avatar_url, '') ELSE '' END AS avatar_url,
                CASE WHEN u.visible THEN COALESCE(u.profile_url, '') ELSE '' END AS profile_url,
                COALESCE(fs.total_commands, 0) AS total_commands,
                COALESCE(fs.total_input_tokens, 0) AS total_input_tokens,
                COALESCE(fs.total_output_tokens, 0) AS total_output_tokens,
                COALESCE(fs.savings_pct, 0.0) AS savings_pct,
                COALESCE(fs.total_raw_tokens, 0) AS total_raw_tokens,
                (SELECT COUNT(*)::BIGINT FROM filter_tests
                 WHERE filter_hash = f.content_hash) AS test_count
         FROM filters f
         JOIN users u ON u.id = f.author_id
         LEFT JOIN filter_stats fs ON fs.filter_hash = f.content_hash
         ORDER BY COALESCE(fs.savings_pct, 0.0)
                  * (1.0 + LN(CAST(COALESCE(fs.total_commands, 0) + 1 AS FLOAT8))) DESC,
                  f.created_at DESC",
    )
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(map_entry_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::error::AppError::Internal(format!("db mapping: {e}")))
}

async fn fetch_global_stats(pool: &PgPool) -> Result<GlobalStats, crate::error::AppError> {
    let row = sqlx::query(
        "SELECT
            (SELECT COUNT(*)::BIGINT FROM filters) AS total_filters,
            COALESCE(SUM(total_commands), 0)::BIGINT AS total_commands,
            COALESCE(SUM(total_input_tokens), 0)::BIGINT AS total_input_tokens,
            COALESCE(SUM(total_output_tokens), 0)::BIGINT AS total_output_tokens,
            CASE WHEN SUM(total_input_tokens) > 0
                 THEN (SUM(total_input_tokens) - SUM(total_output_tokens))::FLOAT8
                      / SUM(total_input_tokens)::FLOAT8 * 100.0
                 ELSE 0.0 END AS overall_savings_pct,
            COALESCE(SUM(total_raw_tokens), 0)::BIGINT AS total_raw_tokens
         FROM filter_stats",
    )
    .fetch_one(pool)
    .await?;

    map_global_stats_row(&row)
        .map_err(|e| crate::error::AppError::Internal(format!("db mapping: {e}")))
}

fn map_global_stats_row(row: &sqlx::postgres::PgRow) -> Result<GlobalStats, sqlx::Error> {
    Ok(GlobalStats {
        total_filters: row.try_get("total_filters")?,
        total_commands: row.try_get("total_commands")?,
        total_input_tokens: row.try_get("total_input_tokens")?,
        total_output_tokens: row.try_get("total_output_tokens")?,
        overall_savings_pct: row.try_get("overall_savings_pct")?,
        total_raw_tokens: row.try_get("total_raw_tokens")?,
    })
}

fn map_entry_row(row: &sqlx::postgres::PgRow) -> Result<CatalogEntry, sqlx::Error> {
    Ok(CatalogEntry {
        content_hash: row.try_get("content_hash")?,
        command_pattern: row.try_get("command_pattern")?,
        canonical_command: row.try_get("canonical_command")?,
        is_stdlib: row.try_get("is_stdlib")?,
        created_at: row.try_get("created_at")?,
        test_count: row.try_get("test_count")?,
        safety_passed: row.try_get("safety_passed")?,
        stdlib_version: row.try_get("stdlib_version")?,
        author: CatalogAuthor {
            username: row.try_get("username")?,
            avatar_url: row.try_get("avatar_url")?,
            profile_url: row.try_get("profile_url")?,
        },
        stats: CatalogFilterStats {
            total_commands: row.try_get("total_commands")?,
            total_input_tokens: row.try_get("total_input_tokens")?,
            total_output_tokens: row.try_get("total_output_tokens")?,
            savings_pct: row.try_get("savings_pct")?,
            total_raw_tokens: row.try_get("total_raw_tokens")?,
        },
    })
}

// ── R2 write functions ───────────────────────────────────────────────────────

/// Serialize and write the catalog index to R2.
///
/// # Errors
///
/// Returns an error if serialization or R2 storage fails.
pub async fn write_catalog_to_r2(
    storage: &dyn StorageClient,
    index: &CatalogIndex,
) -> anyhow::Result<()> {
    let json = serde_json::to_vec(index)?;
    storage.put(catalog_index_key(), json).await?;
    Ok(())
}

/// Serialize and write per-filter metadata to R2.
///
/// # Errors
///
/// Returns an error if serialization or R2 storage fails.
pub async fn write_filter_metadata_to_r2(
    storage: &dyn StorageClient,
    hash: &str,
    entry: &CatalogEntry,
) -> anyhow::Result<()> {
    let json = serde_json::to_vec(entry)?;
    storage.put(&filter_metadata_key(hash), json).await?;
    Ok(())
}

/// Write filter examples (before/after pairs) to R2.
///
/// Always overwrites — examples regenerate when tests change.
///
/// # Errors
///
/// Returns an error if the R2 storage call fails.
pub async fn write_examples_to_r2(
    storage: &dyn StorageClient,
    hash: &str,
    examples_json: Vec<u8>,
) -> anyhow::Result<()> {
    storage
        .put(&filter_examples_key(hash), examples_json)
        .await?;
    Ok(())
}

// ── Orchestrator ─────────────────────────────────────────────────────────────

/// Full catalog refresh: build index from DB and write all artifacts to R2.
///
/// # Errors
///
/// Returns an error if the DB query or R2 write fails.
pub async fn refresh_catalog(
    pool: &PgPool,
    storage: &dyn StorageClient,
) -> Result<CatalogIndex, crate::error::AppError> {
    let index = build_catalog_index(pool).await?;

    write_catalog_to_r2(storage, &index)
        .await
        .map_err(|e| crate::error::AppError::Internal(format!("R2 write failed: {e}")))?;

    // Best-effort: write per-filter metadata, logging failures instead of aborting.
    // A partial write is acceptable — the admin refresh endpoint can reconcile.
    for entry in &index.filters {
        if let Err(e) = write_filter_metadata_to_r2(storage, &entry.content_hash, entry).await {
            tracing::warn!(
                hash = %entry.content_hash,
                "R2 metadata write failed (continuing): {e}"
            );
        }
    }

    Ok(index)
}

// ── Fire-and-forget helpers ──────────────────────────────────────────────────

/// Spawn a background task to update a single filter's metadata and the catalog index.
///
/// Logs warnings on failure — best-effort, admin refresh endpoint catches missed writes.
pub fn spawn_catalog_update(
    pool: PgPool,
    storage: std::sync::Arc<dyn StorageClient>,
    hash: String,
) {
    tokio::spawn(async move {
        if let Err(e) = update_filter_and_index(&pool, &*storage, &hash).await {
            tracing::warn!(hash = %hash, "catalog update failed: {e}");
        }
    });
}

/// Spawn a background task to update metadata for multiple filters and rebuild the index.
///
/// Per-filter metadata writes that fail are logged and skipped so one bad hash
/// doesn't block the rest. When `warn_on_missing` is `false`, missing filters
/// are logged at `debug` level (useful for sync, where hashes may reference
/// local-only unpublished filters).
pub fn spawn_batch_catalog_update(
    pool: PgPool,
    storage: std::sync::Arc<dyn StorageClient>,
    hashes: Vec<String>,
    warn_on_missing: bool,
) {
    tokio::spawn(async move {
        for hash in &hashes {
            match build_filter_metadata(&pool, hash).await {
                Ok(entry) => {
                    if let Err(e) = write_filter_metadata_to_r2(&*storage, hash, &entry).await {
                        tracing::warn!(hash = %hash, "batch catalog metadata write failed: {e}");
                    }
                }
                Err(e) => {
                    if warn_on_missing {
                        tracing::warn!(hash = %hash, "batch catalog metadata build failed: {e}");
                    } else {
                        tracing::debug!(hash = %hash, "batch catalog metadata build skipped: {e}");
                    }
                }
            }
        }
        match build_catalog_index(&pool).await {
            Ok(index) => {
                if let Err(e) = write_catalog_to_r2(&*storage, &index).await {
                    tracing::warn!("batch catalog index write failed: {e}");
                }
            }
            Err(e) => tracing::warn!("batch catalog index build failed: {e}"),
        }
    });
}

async fn update_filter_and_index(
    pool: &PgPool,
    storage: &dyn StorageClient,
    hash: &str,
) -> Result<(), crate::error::AppError> {
    let entry = build_filter_metadata(pool, hash).await?;
    write_filter_metadata_to_r2(storage, hash, &entry)
        .await
        .map_err(|e| crate::error::AppError::Internal(format!("R2 metadata write: {e}")))?;

    let index = build_catalog_index(pool).await?;
    write_catalog_to_r2(storage, &index)
        .await
        .map_err(|e| crate::error::AppError::Internal(format!("R2 index write: {e}")))?;

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::storage::mock::InMemoryStorageClient;

    #[test]
    fn catalog_index_key_is_correct() {
        assert_eq!(catalog_index_key(), "catalog/index.json");
    }

    #[test]
    fn filter_metadata_key_format() {
        assert_eq!(
            filter_metadata_key("abc123"),
            "filters/abc123/metadata.json"
        );
    }

    #[test]
    fn filter_examples_key_format() {
        assert_eq!(
            filter_examples_key("abc123"),
            "filters/abc123/examples.json"
        );
    }

    #[test]
    fn serde_round_trip_catalog_entry() {
        let entry = CatalogEntry {
            content_hash: "deadbeef".to_string(),
            command_pattern: "git push".to_string(),
            canonical_command: "git".to_string(),
            author: CatalogAuthor {
                username: "alice".to_string(),
                avatar_url: "https://github.com/alice.png".to_string(),
                profile_url: "https://github.com/alice".to_string(),
            },
            is_stdlib: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            test_count: 3,
            safety_passed: true,
            stdlib_version: None,
            stats: CatalogFilterStats {
                total_commands: 100,
                total_input_tokens: 5000,
                total_output_tokens: 2000,
                savings_pct: 60.0,
                total_raw_tokens: 5000,
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: CatalogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
    }

    #[test]
    fn serde_round_trip_catalog_index() {
        let index = CatalogIndex {
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            version: 1,
            filters: vec![],
            global_stats: GlobalStats {
                total_filters: 0,
                total_commands: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                overall_savings_pct: 0.0,
                total_raw_tokens: 0,
            },
        };
        let json = serde_json::to_string(&index).unwrap();
        let deserialized: CatalogIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(index, deserialized);
    }

    #[tokio::test]
    async fn write_catalog_to_r2_stores_valid_json() {
        let storage = InMemoryStorageClient::new();
        let index = CatalogIndex {
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            version: 1,
            filters: vec![],
            global_stats: GlobalStats {
                total_filters: 0,
                total_commands: 0,
                total_input_tokens: 0,
                total_output_tokens: 0,
                overall_savings_pct: 0.0,
                total_raw_tokens: 0,
            },
        };

        write_catalog_to_r2(&storage, &index).await.unwrap();

        let bytes = storage.get("catalog/index.json").await.unwrap().unwrap();
        let deserialized: CatalogIndex = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(deserialized.version, 1);
        assert!(deserialized.filters.is_empty());
    }

    #[tokio::test]
    async fn write_examples_stores_at_correct_key() {
        let storage = InMemoryStorageClient::new();
        let examples_json = br#"{"examples":[],"safety":{"passed":true,"warnings":[]}}"#.to_vec();

        write_examples_to_r2(&storage, "abc123", examples_json)
            .await
            .unwrap();

        let bytes = storage
            .get("filters/abc123/examples.json")
            .await
            .unwrap()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(parsed["safety"]["passed"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn write_filter_metadata_stores_at_correct_key() {
        let storage = InMemoryStorageClient::new();
        let entry = CatalogEntry {
            content_hash: "abc123".to_string(),
            command_pattern: "git push".to_string(),
            canonical_command: "git".to_string(),
            author: CatalogAuthor {
                username: "alice".to_string(),
                avatar_url: "https://github.com/alice.png".to_string(),
                profile_url: "https://github.com/alice".to_string(),
            },
            is_stdlib: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            test_count: 1,
            safety_passed: true,
            stdlib_version: None,
            stats: CatalogFilterStats {
                total_commands: 10,
                total_input_tokens: 500,
                total_output_tokens: 200,
                savings_pct: 60.0,
                total_raw_tokens: 500,
            },
        };

        write_filter_metadata_to_r2(&storage, "abc123", &entry)
            .await
            .unwrap();

        let bytes = storage
            .get("filters/abc123/metadata.json")
            .await
            .unwrap()
            .unwrap();
        let deserialized: CatalogEntry = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(deserialized.content_hash, "abc123");
        assert_eq!(deserialized.author.username, "alice");
    }
}
