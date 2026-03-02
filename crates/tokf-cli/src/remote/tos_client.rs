use serde::{Deserialize, Serialize};

use super::http::Client;

#[derive(Debug, Deserialize)]
pub struct TosInfoResponse {
    pub version: i32,
    pub url: String,
}

#[derive(Debug, Serialize)]
struct AcceptTosRequest {
    version: i32,
}

#[derive(Debug, Deserialize)]
pub struct AcceptTosResponse {
    pub accepted_version: i32,
    pub accepted_at: String,
}

/// Fetch the current `ToS` version and full-text URL from the server.
///
/// This is an unauthenticated endpoint.
///
/// # Errors
///
/// Returns an error on network failure or non-2xx status.
pub fn fetch_tos_info(client: &Client) -> anyhow::Result<TosInfoResponse> {
    client.get("/api/tos")
}

/// Record `ToS` acceptance on the server.
///
/// Requires authentication.
///
/// # Errors
///
/// Returns an error on network failure, non-2xx status, or version mismatch.
pub fn accept_tos(client: &Client, version: i32) -> anyhow::Result<AcceptTosResponse> {
    client.post("/api/tos/accept", &AcceptTosRequest { version })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_tos_info_response() {
        let json = r#"{"version": 1, "url": "https://api.tokf.net/terms"}"#;
        let resp: TosInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.version, 1);
        assert_eq!(resp.url, "https://api.tokf.net/terms");
    }

    #[test]
    fn deserialize_accept_tos_response() {
        let json = r#"{"accepted_version": 1, "accepted_at": "2026-03-02T00:00:00Z"}"#;
        let resp: AcceptTosResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.accepted_version, 1);
        assert_eq!(resp.accepted_at, "2026-03-02T00:00:00Z");
    }
}
