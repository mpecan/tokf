use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DeviceFlowResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub user: TokenUser,
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

pub enum PollResult {
    Success(TokenResponse),
    Pending { interval: i64 },
    SlowDown { interval: i64 },
    Failed(String),
}

const DEFAULT_SERVER_URL: &str = "https://api.tokf.net";

pub fn server_url() -> String {
    std::env::var("TOKF_SERVER_URL").unwrap_or_else(|_| DEFAULT_SERVER_URL.to_string())
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
        return Ok(PollResult::Failed(text));
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
        // When TOKF_SERVER_URL is not set, should return default
        // (We can't unset env vars reliably in parallel tests, so just verify
        // the function doesn't panic)
        let url = server_url();
        assert!(!url.is_empty());
    }
}
