use super::*;
use crate::storage::mock::InMemoryStorageClient;

fn empty_global_stats() -> GlobalStats {
    GlobalStats {
        total_filters: 0,
        total_commands: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        overall_savings_pct: 0.0,
        total_raw_tokens: 0,
    }
}

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
        global_stats: empty_global_stats(),
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
        global_stats: empty_global_stats(),
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

#[test]
fn grouped_catalog_key_is_correct() {
    assert_eq!(grouped_catalog_key(), "catalog/grouped.json");
}

#[test]
fn serde_round_trip_filter_version_info() {
    let info = FilterVersionInfo {
        introduced_at: Some("0.2.8".to_string()),
        deprecated_at: None,
        successor_hash: None,
        is_current: true,
    };
    let json = serde_json::to_string(&info).unwrap();
    let deserialized: FilterVersionInfo = serde_json::from_str(&json).unwrap();
    assert_eq!(info, deserialized);
}

#[test]
fn serde_round_trip_grouped_catalog() {
    let catalog = GroupedCatalog {
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        version: 2,
        commands: vec![],
        global_stats: empty_global_stats(),
    };
    let json = serde_json::to_string(&catalog).unwrap();
    let deserialized: GroupedCatalog = serde_json::from_str(&json).unwrap();
    assert_eq!(catalog, deserialized);
}

fn make_entry(hash: &str, pattern: &str, is_stdlib: bool, savings: f64) -> CatalogEntry {
    CatalogEntry {
        content_hash: hash.to_string(),
        command_pattern: pattern.to_string(),
        canonical_command: pattern.split_whitespace().next().unwrap_or("").to_string(),
        author: CatalogAuthor {
            username: "test".to_string(),
            avatar_url: String::new(),
            profile_url: String::new(),
        },
        is_stdlib,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        test_count: 1,
        safety_passed: true,
        stats: CatalogFilterStats {
            total_commands: 10,
            total_input_tokens: 500,
            total_output_tokens: 200,
            savings_pct: savings,
            total_raw_tokens: 500,
        },
    }
}

#[test]
fn group_entries_groups_by_command_pattern() {
    let entries = vec![
        VersionedCatalogEntry {
            entry: make_entry("aaa", "git push", true, 60.0),
            version_info: None,
        },
        VersionedCatalogEntry {
            entry: make_entry("bbb", "git push", false, 40.0),
            version_info: None,
        },
        VersionedCatalogEntry {
            entry: make_entry("ccc", "cargo build", true, 50.0),
            version_info: None,
        },
    ];
    let groups = group_entries(entries);
    assert_eq!(groups.len(), 2);
    let push_group = groups
        .iter()
        .find(|g| g.command_pattern == "git push")
        .unwrap();
    assert_eq!(push_group.filter_count, 2);
    assert_eq!(push_group.filters.len(), 2);
}

#[test]
fn normalize_command_pattern_strips_trailing_wildcard() {
    assert_eq!(
        normalize_command_pattern("gh issue view *"),
        "gh issue view"
    );
    assert_eq!(
        normalize_command_pattern("cargo install *"),
        "cargo install"
    );
    assert_eq!(normalize_command_pattern("git push"), "git push");
    assert_eq!(normalize_command_pattern("npm run *"), "npm run");
    // Only strips trailing " *", not internal wildcards
    assert_eq!(normalize_command_pattern("* something"), "* something");
}

#[test]
fn group_entries_merges_wildcard_variants() {
    let entries = vec![
        VersionedCatalogEntry {
            entry: make_entry("aaa", "gh issue view *", true, 60.0),
            version_info: None,
        },
        VersionedCatalogEntry {
            entry: make_entry("bbb", "gh issue view", false, 40.0),
            version_info: None,
        },
    ];
    let groups = group_entries(entries);
    assert_eq!(groups.len(), 1, "wildcard and non-wildcard should merge");
    assert_eq!(groups[0].command_pattern, "gh issue view");
    assert_eq!(groups[0].filter_count, 2);
}

#[test]
fn select_primary_prefers_current_stdlib() {
    let stdlib = VersionedCatalogEntry {
        entry: make_entry("aaa", "git push", true, 30.0),
        version_info: None,
    };
    let community = VersionedCatalogEntry {
        entry: make_entry("bbb", "git push", false, 90.0),
        version_info: None,
    };
    // stdlib (non-deprecated) should be primary even with lower score
    let entries = [community, stdlib];
    let primary = select_primary(&entries);
    assert_eq!(primary.entry.content_hash, "aaa");
}

#[test]
fn select_primary_skips_deprecated_stdlib() {
    let deprecated = VersionedCatalogEntry {
        entry: make_entry("old", "git push", true, 30.0),
        version_info: Some(FilterVersionInfo {
            introduced_at: Some("0.1.0".to_string()),
            deprecated_at: Some("0.2.0".to_string()),
            successor_hash: Some("new".to_string()),
            is_current: false,
        }),
    };
    let community = VersionedCatalogEntry {
        entry: make_entry("bbb", "git push", false, 90.0),
        version_info: None,
    };
    let entries = [deprecated, community];
    let primary = select_primary(&entries);
    assert_eq!(primary.entry.content_hash, "bbb");
}

#[test]
fn version_info_none_fields_omitted_in_json() {
    let info = FilterVersionInfo {
        introduced_at: Some("0.2.8".to_string()),
        deprecated_at: None,
        successor_hash: None,
        is_current: true,
    };
    let json = serde_json::to_string(&info).unwrap();
    assert!(!json.contains("deprecated_at"));
    assert!(!json.contains("successor_hash"));
    assert!(json.contains("introduced_at"));
}

#[tokio::test]
async fn write_grouped_catalog_stores_valid_json() {
    let storage = InMemoryStorageClient::new();
    let catalog = GroupedCatalog {
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        version: 2,
        commands: vec![],
        global_stats: empty_global_stats(),
    };

    write_grouped_catalog_to_r2(&storage, &catalog)
        .await
        .unwrap();

    let bytes = storage.get("catalog/grouped.json").await.unwrap().unwrap();
    let deserialized: GroupedCatalog = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(deserialized.version, 2);
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
