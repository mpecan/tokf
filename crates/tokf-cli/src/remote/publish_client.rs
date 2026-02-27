use serde::Deserialize;

use super::require_success;

#[derive(Debug, Deserialize)]
pub struct PublishResponse {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub registry_url: String,
}

/// Publish a filter and optional test files to the community registry.
///
/// Returns `(is_new, response)`:
/// - `is_new = true` when the server returns `201 Created` (first upload).
/// - `is_new = false` when the server returns `200 OK` (hash already exists).
///
/// The `client` should have connection and request timeouts configured.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn publish_filter(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    filter_bytes: Vec<u8>,
    test_files: Vec<(String, Vec<u8>)>,
) -> anyhow::Result<(bool, PublishResponse)> {
    let url = format!("{base_url}/api/filters");

    let filter_part = reqwest::blocking::multipart::Part::bytes(filter_bytes)
        .mime_str("application/toml")
        .map_err(|e| anyhow::anyhow!("invalid MIME type: {e}"))?;

    let mut form = reqwest::blocking::multipart::Form::new().part("filter", filter_part);
    form = form.part(
        "mit_license_accepted",
        reqwest::blocking::multipart::Part::text("true"),
    );
    for (name, bytes) in test_files {
        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .mime_str("application/toml")
            .map_err(|e| anyhow::anyhow!("invalid MIME type for {name}: {e}"))?;
        form = form.part(format!("test/{name}"), part);
    }

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .multipart(form)
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let is_new = resp.status() == reqwest::StatusCode::CREATED;
    let resp = require_success(resp)?;
    let response = resp
        .json::<PublishResponse>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))?;
    Ok((is_new, response))
}

#[derive(Debug, Deserialize)]
pub struct UpdateTestsResponse {
    pub content_hash: String,
    pub command_pattern: String,
    pub author: String,
    pub test_count: usize,
    pub registry_url: String,
}

/// Update the test suite for an already-published filter.
///
/// Sends a PUT request with multipart `test/<filename>` fields only.
/// Only the original author is allowed to update tests.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status (403 = not author, 404 = filter not found), or the response
/// body cannot be deserialized.
pub fn update_tests(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    content_hash: &str,
    test_files: Vec<(String, Vec<u8>)>,
) -> anyhow::Result<UpdateTestsResponse> {
    let url = format!("{base_url}/api/filters/{content_hash}/tests");

    let mut form = reqwest::blocking::multipart::Form::new();
    for (name, bytes) in test_files {
        let part = reqwest::blocking::multipart::Part::bytes(bytes)
            .mime_str("application/toml")
            .map_err(|e| anyhow::anyhow!("invalid MIME type for {name}: {e}"))?;
        form = form.part(format!("test/{name}"), part);
    }

    let resp = client
        .put(&url)
        .header("Authorization", format!("Bearer {token}"))
        .multipart(form)
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!("you are not the author of this filter");
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("filter not found in registry (hash: {content_hash})");
    }
    let resp = require_success(resp)?;
    resp.json::<UpdateTestsResponse>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_publish_response() {
        let json = r#"{
            "content_hash": "abc123def456",
            "command_pattern": "git push",
            "author": "alice",
            "registry_url": "filters/abc123def456/filter.toml"
        }"#;
        let resp: PublishResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content_hash, "abc123def456");
        assert_eq!(resp.command_pattern, "git push");
        assert_eq!(resp.author, "alice");
        assert_eq!(resp.registry_url, "filters/abc123def456/filter.toml");
    }

    #[test]
    fn deserialize_update_tests_response() {
        let json = r#"{
            "content_hash": "abc123def456",
            "command_pattern": "git push",
            "author": "alice",
            "test_count": 3,
            "registry_url": "https://registry.tokf.net/filters/abc123def456"
        }"#;
        let resp: UpdateTestsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.content_hash, "abc123def456");
        assert_eq!(resp.command_pattern, "git push");
        assert_eq!(resp.author, "alice");
        assert_eq!(resp.test_count, 3);
        assert_eq!(
            resp.registry_url,
            "https://registry.tokf.net/filters/abc123def456"
        );
    }

    #[test]
    fn deserialize_duplicate_response() {
        // Same shape — 200 and 201 return identical JSON structure
        let json = r#"{
            "content_hash": "deadbeef",
            "command_pattern": "cargo build",
            "author": "bob",
            "registry_url": "filters/deadbeef/filter.toml"
        }"#;
        let resp: PublishResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.author, "bob");
        assert_eq!(resp.registry_url, "filters/deadbeef/filter.toml");
    }
}
