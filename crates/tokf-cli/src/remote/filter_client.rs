use serde::{Deserialize, Serialize};

use super::http::Client;

#[derive(Debug, Deserialize, Serialize)]
pub struct FilterSummary {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub savings_pct: f64,
    pub total_commands: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub is_stdlib: bool,
}

#[derive(Debug, Deserialize)]
pub struct FilterDetails {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub savings_pct: f64,
    pub total_commands: i64,
    pub created_at: String,
    pub test_count: i64,
    pub registry_url: String,
    #[serde(default)]
    pub is_stdlib: bool,
}

#[derive(Debug, Deserialize)]
pub struct TestFilePayload {
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct DownloadedFilter {
    pub filter_toml: String,
    pub test_files: Vec<TestFilePayload>,
}

/// Search the community filter registry.
///
/// Returns up to `limit` filters matching the `query` substring.
/// Pass an empty `query` to return all filters.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn search_filters(
    client: &Client,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<FilterSummary>> {
    let limit_str = limit.to_string();
    client.get_with_query("/api/filters", &[("q", query), ("limit", &limit_str)])
}

/// Get details for a specific filter by content hash.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn get_filter(client: &Client, hash: &str) -> anyhow::Result<FilterDetails> {
    client.get(&format!("/api/filters/{hash}"))
}

/// Download a filter's TOML and test files by content hash.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn download_filter(client: &Client, hash: &str) -> anyhow::Result<DownloadedFilter> {
    client.get(&format!("/api/filters/{hash}/download"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_filter_summary() {
        let json = r#"{
            "content_hash": "abc123def456abc123def456abc123def456abc123def456abc123def456abc1",
            "command_pattern": "git push",
            "author": "alice",
            "savings_pct": 42.3,
            "total_commands": 1234
        }"#;
        let summary: FilterSummary = serde_json::from_str(json).unwrap();
        assert_eq!(
            summary.content_hash,
            "abc123def456abc123def456abc123def456abc123def456abc123def456abc1"
        );
        assert_eq!(summary.command_pattern, "git push");
        assert_eq!(summary.author, "alice");
        assert!((summary.savings_pct - 42.3).abs() < 0.001);
        assert_eq!(summary.total_commands, 1234);
        assert_eq!(summary.created_at, "", "created_at defaults to empty");
    }

    #[test]
    fn deserialize_filter_summary_with_created_at() {
        let json = r#"{
            "content_hash": "abc123",
            "command_pattern": "git push",
            "author": "alice",
            "savings_pct": 0.0,
            "total_commands": 0,
            "created_at": "2026-02-26T00:00:00"
        }"#;
        let summary: FilterSummary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.created_at, "2026-02-26T00:00:00");
    }

    #[test]
    fn deserialize_downloaded_filter() {
        let json = r#"{
            "filter_toml": "command = \"git push\"\n",
            "test_files": [
                {"filename": "basic.toml", "content": "name = \"basic\"\n"},
                {"filename": "edge.toml", "content": "name = \"edge\"\n"}
            ]
        }"#;
        let dl: DownloadedFilter = serde_json::from_str(json).unwrap();
        assert!(dl.filter_toml.contains("git push"));
        assert_eq!(dl.test_files.len(), 2);
        assert_eq!(dl.test_files[0].filename, "basic.toml");
        assert_eq!(dl.test_files[1].filename, "edge.toml");
    }

    #[test]
    fn deserialize_downloaded_filter_no_tests() {
        let json = r#"{"filter_toml": "command = \"cargo build\"\n", "test_files": []}"#;
        let dl: DownloadedFilter = serde_json::from_str(json).unwrap();
        assert!(dl.test_files.is_empty());
    }

    #[test]
    fn deserialize_filter_summary_with_is_stdlib() {
        let json = r#"{
            "content_hash": "abc123",
            "command_pattern": "git push",
            "author": "alice",
            "savings_pct": 0.0,
            "total_commands": 0,
            "is_stdlib": true
        }"#;
        let summary: FilterSummary = serde_json::from_str(json).unwrap();
        assert!(summary.is_stdlib);
    }

    #[test]
    fn deserialize_filter_summary_is_stdlib_defaults_false() {
        let json = r#"{
            "content_hash": "abc123",
            "command_pattern": "git push",
            "author": "alice",
            "savings_pct": 0.0,
            "total_commands": 0
        }"#;
        let summary: FilterSummary = serde_json::from_str(json).unwrap();
        assert!(!summary.is_stdlib);
    }
}
