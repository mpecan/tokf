use std::collections::BTreeMap;

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

// ── Versioned catalog types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct FilterVersionInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_hash: Option<String>,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct VersionedCatalogEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_info: Option<FilterVersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct CommandGroup {
    pub command_pattern: String,
    pub canonical_command: String,
    pub primary_hash: String,
    pub primary_stats: CatalogFilterStats,
    #[cfg_attr(test, ts(type = "number"))]
    pub filter_count: usize,
    pub filters: Vec<VersionedCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export, export_to = "../generated/"))]
pub struct GroupedCatalog {
    pub generated_at: String,
    pub version: u32,
    pub commands: Vec<CommandGroup>,
    pub global_stats: GlobalStats,
}

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

/// R2 key for the grouped catalog.
pub const fn grouped_catalog_key() -> &'static str {
    "catalog/grouped.json"
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

/// Build the grouped catalog from the database.
///
/// Groups filters by `command_pattern`, selects a primary filter per group
/// (current stdlib preferred, then highest relevance score), and attaches
/// version info where available.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub async fn build_grouped_catalog(
    pool: &PgPool,
) -> Result<GroupedCatalog, crate::error::AppError> {
    let entries = fetch_all_versioned_entries(pool).await?;
    let global_stats = fetch_global_stats(pool).await?;
    let commands = group_entries(entries);

    Ok(GroupedCatalog {
        generated_at: chrono::Utc::now().to_rfc3339(),
        version: 2,
        commands,
        global_stats,
    })
}

/// Normalize a command pattern for grouping: strip trailing wildcard ` *`.
///
/// E.g. `"gh issue view *"` → `"gh issue view"`, so that `gh issue view *`
/// and `gh issue view` end up in the same group.
fn normalize_command_pattern(pattern: &str) -> &str {
    pattern.strip_suffix(" *").unwrap_or(pattern)
}

/// Group versioned entries by normalized `command_pattern` and select primary filter per group.
fn group_entries(entries: Vec<VersionedCatalogEntry>) -> Vec<CommandGroup> {
    let mut groups: BTreeMap<String, Vec<VersionedCatalogEntry>> = BTreeMap::new();
    for entry in entries {
        let key = normalize_command_pattern(&entry.entry.command_pattern).to_string();
        groups.entry(key).or_default().push(entry);
    }

    let mut commands: Vec<CommandGroup> = groups
        .into_iter()
        .map(|(command_pattern, filters)| {
            let primary = select_primary(&filters);
            CommandGroup {
                canonical_command: primary.entry.canonical_command.clone(),
                primary_hash: primary.entry.content_hash.clone(),
                primary_stats: primary.entry.stats.clone(),
                filter_count: filters.len(),
                filters,
                command_pattern,
            }
        })
        .collect();

    // Sort groups by primary filter relevance (same as flat catalog)
    #[allow(clippy::cast_precision_loss)]
    commands.sort_by(|a, b| {
        let score =
            |s: &CatalogFilterStats| s.savings_pct * (1.0 + ((s.total_commands + 1) as f64).ln());
        score(&b.primary_stats)
            .partial_cmp(&score(&a.primary_stats))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    commands
}

/// Select the primary filter from a group: current stdlib first, then first
/// non-deprecated entry by score, then first entry overall.
fn select_primary(filters: &[VersionedCatalogEntry]) -> &VersionedCatalogEntry {
    let is_deprecated = |f: &&VersionedCatalogEntry| -> bool {
        f.version_info
            .as_ref()
            .is_some_and(|v| v.deprecated_at.is_some())
    };

    // Prefer current (non-deprecated) stdlib.
    if let Some(current_stdlib) = filters
        .iter()
        .find(|f| f.entry.is_stdlib && !is_deprecated(f))
    {
        return current_stdlib;
    }

    // Fall back to first non-deprecated entry (by pre-sorted score).
    filters
        .iter()
        .find(|f| !is_deprecated(f))
        .unwrap_or(&filters[0])
}

async fn fetch_all_versioned_entries(
    pool: &PgPool,
) -> Result<Vec<VersionedCatalogEntry>, crate::error::AppError> {
    let rows = sqlx::query(
        "SELECT f.content_hash, f.command_pattern, f.canonical_command, f.is_stdlib,
                f.created_at::TEXT AS created_at,
                f.safety_passed,
                f.introduced_at, f.deprecated_at, f.successor_hash,
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
        .map(map_versioned_entry_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::error::AppError::Internal(format!("db mapping: {e}")))
}

fn map_versioned_entry_row(
    row: &sqlx::postgres::PgRow,
) -> Result<VersionedCatalogEntry, sqlx::Error> {
    let entry = map_entry_row(row)?;
    let introduced_at: Option<String> = row.try_get("introduced_at")?;
    let deprecated_at: Option<String> = row.try_get("deprecated_at")?;
    let successor_hash: Option<String> = row.try_get("successor_hash")?;

    let version_info = if introduced_at.is_some() || deprecated_at.is_some() {
        Some(FilterVersionInfo {
            is_current: entry.is_stdlib && deprecated_at.is_none(),
            introduced_at,
            deprecated_at,
            successor_hash,
        })
    } else {
        None
    };

    Ok(VersionedCatalogEntry {
        entry,
        version_info,
    })
}

async fn fetch_all_entries(pool: &PgPool) -> Result<Vec<CatalogEntry>, crate::error::AppError> {
    let versioned = fetch_all_versioned_entries(pool).await?;
    Ok(versioned.into_iter().map(|v| v.entry).collect())
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

/// Serialize and write the grouped catalog to R2.
///
/// # Errors
///
/// Returns an error if serialization or R2 storage fails.
pub async fn write_grouped_catalog_to_r2(
    storage: &dyn StorageClient,
    catalog: &GroupedCatalog,
) -> anyhow::Result<()> {
    let json = serde_json::to_vec(catalog)?;
    storage.put(grouped_catalog_key(), json).await?;
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

    // Best-effort: write grouped catalog alongside the flat one.
    match build_grouped_catalog(pool).await {
        Ok(grouped) => {
            if let Err(e) = write_grouped_catalog_to_r2(storage, &grouped).await {
                tracing::warn!("grouped catalog R2 write failed: {e}");
            }
        }
        Err(e) => tracing::warn!("grouped catalog build failed: {e}"),
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
        // Best-effort: rebuild grouped catalog too.
        match build_grouped_catalog(&pool).await {
            Ok(grouped) => {
                if let Err(e) = write_grouped_catalog_to_r2(&*storage, &grouped).await {
                    tracing::warn!("batch grouped catalog write failed: {e}");
                }
            }
            Err(e) => tracing::warn!("batch grouped catalog build failed: {e}"),
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
#[path = "catalog_tests.rs"]
mod tests;
