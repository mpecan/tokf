use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct SyncEvent {
    pub id: i64,
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub command_count: i32,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncRequest {
    pub machine_id: String,
    pub last_event_id: i64,
    pub events: Vec<SyncEvent>,
}

#[derive(Debug, Deserialize)]
pub struct SyncResponse {
    pub accepted: usize,
    pub cursor: i64,
}

/// Send a batch of usage events to the remote server.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success status,
/// or the response body cannot be deserialized.
pub fn sync_events(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    req: &SyncRequest,
) -> anyhow::Result<SyncResponse> {
    let url = format!("{base_url}/api/sync");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(req)
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "server returned HTTP 401 Unauthorized — run `tokf auth login` to re-authenticate"
        );
    }
    if !status.is_success() {
        let text = resp
            .text()
            .unwrap_or_else(|_| "<unreadable body>".to_string());
        anyhow::bail!("server returned HTTP {status}: {text}");
    }

    let response = resp
        .json::<SyncResponse>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))?;
    Ok(response)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn serialize_sync_request() {
        let req = SyncRequest {
            machine_id: "00000000-0000-0000-0000-000000000001".to_string(),
            last_event_id: 5,
            events: vec![SyncEvent {
                id: 6,
                filter_name: Some("git/push".to_string()),
                filter_hash: None,
                input_tokens: 1000,
                output_tokens: 200,
                command_count: 1,
                recorded_at: "2026-01-01T00:00:00Z".to_string(),
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"machine_id\""));
        assert!(json.contains("\"last_event_id\":5"));
        assert!(json.contains("\"input_tokens\":1000"));
    }

    #[test]
    fn deserialize_sync_response() {
        let json = r#"{"accepted":3,"cursor":8}"#;
        let resp: SyncResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.accepted, 3);
        assert_eq!(resp.cursor, 8);
    }
}
