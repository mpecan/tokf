use std::fmt;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct DeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: i64,
    pub interval: i64,
}

// Manual Debug impl to redact the device_code secret
impl fmt::Debug for DeviceFlowResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceFlowResponse")
            .field("device_code", &"[REDACTED]")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("verification_uri_complete", &self.verification_uri_complete)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish()
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: TokenUser,
}

// Manual Debug impl to redact the access_token secret
impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"[REDACTED]")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("user", &self.user)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenUser {
    pub id: i64,
    pub username: String,
    pub avatar_url: String,
}

#[derive(Debug, Deserialize)]
pub struct PendingResponse {
    pub error: String,
    pub interval: Option<i64>,
}

/// Result of a single token poll attempt.
pub enum PollResult {
    Success(TokenResponse),
    Pending { interval: i64 },
    SlowDown { interval: i64 },
    Failed(String),
}

const DEFAULT_SERVER_URL: &str = "https://api.tokf.net";

/// Returns the tokf server URL from `TOKF_SERVER_URL` or the default.
pub fn server_url() -> String {
    std::env::var("TOKF_SERVER_URL").unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string())
}

/// Returns `true` if the URL uses HTTPS or targets localhost.
pub fn is_secure_url(url: &str) -> bool {
    if url.starts_with("https://") {
        return true;
    }
    // Allow http:// for localhost development
    url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1")
}

/// Returns `true` if the URI is safe to open in a browser (https or localhost http).
pub fn is_safe_browser_uri(uri: &str) -> bool {
    if uri.starts_with("https://") {
        return true;
    }
    uri.starts_with("http://localhost") || uri.starts_with("http://127.0.0.1")
}

/// Start the device authorization flow by calling `POST /api/auth/device`.
///
/// # Errors
///
/// Returns an error if the server is unreachable or returns a non-success status.
pub fn initiate_device_flow(
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> anyhow::Result<DeviceFlowResponse> {
    let url = format!("{base_url}/api/auth/device");
    let resp = client
        .post(&url)
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("server returned HTTP {status}");
    }

    let body = resp
        .json::<DeviceFlowResponse>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))?;
    Ok(body)
}

/// Poll for a completed device authorization via `POST /api/auth/token`.
///
/// # Errors
///
/// Returns an error if the server is unreachable or returns a 5xx status.
pub fn poll_token(
    client: &reqwest::blocking::Client,
    base_url: &str,
    device_code: &str,
) -> anyhow::Result<PollResult> {
    let url = format!("{base_url}/api/auth/token");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "device_code": device_code }))
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let status = resp.status();

    // 4xx errors from the server indicate denied/expired
    if status.is_client_error() {
        let text = resp.text().unwrap_or_default();
        let msg = extract_error_message(&text);
        return Ok(PollResult::Failed(msg));
    }

    if !status.is_success() {
        anyhow::bail!("server returned HTTP {status}");
    }

    let text = resp.text()?;

    // Try to parse as TokenResponse first (success case)
    if let Ok(token_resp) = serde_json::from_str::<TokenResponse>(&text) {
        return Ok(PollResult::Success(token_resp));
    }

    // Otherwise parse as PendingResponse
    let pending: PendingResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("unexpected response from server: {e}"))?;

    match pending.error.as_str() {
        "authorization_pending" => Ok(PollResult::Pending {
            interval: pending.interval.unwrap_or(5),
        }),
        "slow_down" => Ok(PollResult::SlowDown {
            interval: pending.interval.unwrap_or(10),
        }),
        other => Ok(PollResult::Failed(other.to_string())),
    }
}

/// Extract a human-readable error message from a server response body.
/// Tries to parse JSON `error_description` or `error` fields first,
/// falls back to the raw text (truncated and sanitized).
fn extract_error_message(body: &str) -> String {
    // Try JSON error fields
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(desc) = obj.get("error_description").and_then(|v| v.as_str()) {
            return sanitize_error_text(desc);
        }
        if let Some(err) = obj.get("error").and_then(|v| v.as_str()) {
            return sanitize_error_text(err);
        }
    }
    sanitize_error_text(body)
}

/// Truncate to 256 chars and strip control characters.
fn sanitize_error_text(text: &str) -> String {
    let truncated = if text.len() > 256 {
        format!("{}...", &text[..256])
    } else {
        text.to_string()
    };
    truncated.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_device_flow_response() {
        let json = r#"{
            "device_code": "dc-abc123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;
        let resp: DeviceFlowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code, "dc-abc123");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_uri, "https://github.com/login/device");
        assert_eq!(resp.expires_in, 900);
        assert_eq!(resp.interval, 5);
        assert!(resp.verification_uri_complete.is_none());
    }

    #[test]
    fn deserialize_device_flow_with_complete_uri() {
        let json = r#"{
            "device_code": "dc-abc123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "verification_uri_complete": "https://github.com/login/device?user_code=ABCD-1234",
            "expires_in": 900,
            "interval": 5
        }"#;
        let resp: DeviceFlowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.verification_uri_complete.as_deref(),
            Some("https://github.com/login/device?user_code=ABCD-1234")
        );
    }

    #[test]
    fn deserialize_token_response() {
        let json = r#"{
            "access_token": "tok_secret",
            "token_type": "bearer",
            "expires_in": 7776000,
            "user": {
                "id": 42,
                "username": "octocat",
                "avatar_url": "https://avatars.githubusercontent.com/u/42"
            }
        }"#;
        let resp: TokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token, "tok_secret");
        assert_eq!(resp.token_type, "bearer");
        assert_eq!(resp.expires_in, 7_776_000);
        assert_eq!(resp.user.id, 42);
        assert_eq!(resp.user.username, "octocat");
    }

    #[test]
    fn deserialize_pending_response() {
        let json = r#"{"error": "authorization_pending"}"#;
        let resp: PendingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error, "authorization_pending");
        assert!(resp.interval.is_none());
    }

    #[test]
    fn deserialize_slow_down_response() {
        let json = r#"{"error": "slow_down", "interval": 10}"#;
        let resp: PendingResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error, "slow_down");
        assert_eq!(resp.interval, Some(10));
    }

    #[test]
    fn server_url_default() {
        // When TOKF_SERVER_URL is not set, should return default.
        // Can't unset env vars reliably in parallel tests, so just verify
        // the function doesn't panic and returns a non-empty string.
        let url = server_url();
        assert!(!url.is_empty());
    }

    #[test]
    fn debug_redacts_device_code() {
        let resp = DeviceFlowResponse {
            device_code: "super-secret".to_string(),
            user_code: "ABCD-1234".to_string(),
            verification_uri: "https://example.com".to_string(),
            verification_uri_complete: None,
            expires_in: 900,
            interval: 5,
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret"));
    }

    #[test]
    fn is_secure_url_checks() {
        assert!(is_secure_url("https://api.tokf.net"));
        assert!(is_secure_url("http://localhost:8080"));
        assert!(is_secure_url("http://127.0.0.1:8080"));
        assert!(!is_secure_url("http://evil.example.com"));
    }

    #[test]
    fn is_safe_browser_uri_checks() {
        assert!(is_safe_browser_uri("https://github.com/login/device"));
        assert!(is_safe_browser_uri("http://localhost:3000/auth"));
        assert!(!is_safe_browser_uri("file:///etc/passwd"));
        assert!(!is_safe_browser_uri("http://evil.com/phish"));
        assert!(!is_safe_browser_uri("ssh://something"));
    }

    #[test]
    fn extract_error_message_from_json() {
        let body = r#"{"error":"access_denied","error_description":"The user denied"}"#;
        assert_eq!(extract_error_message(body), "The user denied");
    }

    #[test]
    fn extract_error_message_from_json_error_field() {
        let body = r#"{"error":"expired_token"}"#;
        assert_eq!(extract_error_message(body), "expired_token");
    }

    #[test]
    fn extract_error_message_raw_text() {
        assert_eq!(
            extract_error_message("Something went wrong"),
            "Something went wrong"
        );
    }

    #[test]
    fn sanitize_error_text_truncates() {
        let long = "x".repeat(300);
        let result = sanitize_error_text(&long);
        assert_eq!(result.len(), 259); // 256 + "..."
    }

    #[test]
    fn sanitize_error_text_strips_control_chars() {
        let text = "hello\x1b[31mworld\x00end";
        let result = sanitize_error_text(text);
        assert_eq!(result, "hello[31mworldend");
    }
}
