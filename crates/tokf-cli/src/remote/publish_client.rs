use reqwest::blocking::multipart::{Form, Part};
use serde::Deserialize;

use super::check_auth_and_rate_limit;
use super::http::Client;

/// Extract the error message from a JSON response body `{"error": "..."}`.
/// Falls back to the raw body text if JSON parsing fails.
fn extract_error_message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or_else(|| body.to_string())
}

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
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn publish_filter(
    client: &Client,
    filter_bytes: &[u8],
    test_files: &[(String, Vec<u8>)],
) -> anyhow::Result<(bool, PublishResponse)> {
    let filter_bytes = filter_bytes.to_vec();
    let test_files = test_files.to_vec();
    let resp = client.post_multipart("/api/filters", move || {
        let mut form = Form::new()
            .part("filter", Part::bytes(filter_bytes))
            .part("mit_license_accepted", Part::text("true"));
        for (name, bytes) in test_files {
            form = form.part(format!("test:{name}"), Part::bytes(bytes));
        }
        form
    })?;

    check_auth_and_rate_limit(&resp)?;

    let is_new = resp.status() == reqwest::StatusCode::CREATED;
    let status = resp.status();

    // Extract test failure details from 400 responses before the generic handler
    if status == reqwest::StatusCode::BAD_REQUEST {
        let body = resp
            .text()
            .map_err(|e| anyhow::anyhow!("could not read response body: {e}"))?;
        let msg = extract_error_message(&body);
        if msg.contains("tests failed") {
            anyhow::bail!(
                "server-side test verification failed:\n\n{msg}\n\n\
                 Hint: run `tokf verify` locally to debug test failures"
            );
        }
        anyhow::bail!("server returned HTTP {status}: {msg}");
    }

    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("server returned HTTP {status}: {body}");
    }

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
/// Sends a PUT request with multipart `test:<filename>` fields only.
/// Only the original author is allowed to update tests.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status (403 = not author, 404 = filter not found), or the response
/// body cannot be deserialized.
pub fn update_tests(
    client: &Client,
    content_hash: &str,
    test_files: &[(String, Vec<u8>)],
) -> anyhow::Result<UpdateTestsResponse> {
    let path = format!("/api/filters/{content_hash}/tests");
    let test_files = test_files.to_vec();
    let resp = client.put_multipart(&path, || {
        let mut form = Form::new();
        for (name, bytes) in &test_files {
            form = form.part(format!("test:{name}"), Part::bytes(bytes.clone()));
        }
        form
    })?;

    check_auth_and_rate_limit(&resp)?;

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        anyhow::bail!("you are not the author of this filter");
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("filter not found in registry (hash: {content_hash})");
    }
    if status == reqwest::StatusCode::BAD_REQUEST {
        let body = resp
            .text()
            .map_err(|e| anyhow::anyhow!("could not read response body: {e}"))?;
        let msg = extract_error_message(&body);
        if msg.contains("tests failed") {
            anyhow::bail!(
                "server-side test verification failed:\n\n{msg}\n\n\
                 Hint: run `tokf verify` locally to debug test failures"
            );
        }
        anyhow::bail!("server returned HTTP {status}: {msg}");
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        anyhow::bail!("server returned HTTP {status}: {body}");
    }
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
