use serde::{Deserialize, Serialize};

use super::require_success;

#[derive(Debug, Deserialize, Serialize)]
pub struct MachineGain {
    pub machine_id: String,
    pub hostname: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalMachineGain {
    pub machine_id: String,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FilterGainEntry {
    pub filter_name: Option<String>,
    pub filter_hash: Option<String>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GainResponse {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub by_machine: Vec<MachineGain>,
    pub by_filter: Vec<FilterGainEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GlobalGainResponse {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_commands: i64,
    pub by_machine: Vec<GlobalMachineGain>,
    pub by_filter: Vec<FilterGainEntry>,
}

/// Fetch the authenticated user's own token savings from the remote server.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn get_gain(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> anyhow::Result<GainResponse> {
    let url = format!("{base_url}/api/gain");
    super::http::authed_get(client, &url, token)
}

/// Fetch global (all-users) token savings from the remote server (public, no auth).
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn get_global_gain(
    client: &reqwest::blocking::Client,
    base_url: &str,
) -> anyhow::Result<GlobalGainResponse> {
    let url = format!("{base_url}/api/gain/global");
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let resp = require_success(resp)?;
    resp.json::<GlobalGainResponse>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_gain_response() {
        let json = r#"{
            "total_input_tokens": 50000,
            "total_output_tokens": 10000,
            "total_commands": 100,
            "by_machine": [{
                "machine_id": "abc-123",
                "hostname": "dev-laptop",
                "total_input_tokens": 50000,
                "total_output_tokens": 10000,
                "total_commands": 100
            }],
            "by_filter": [{
                "filter_name": "git/push",
                "filter_hash": "abc123",
                "total_input_tokens": 30000,
                "total_output_tokens": 5000,
                "total_commands": 60
            }]
        }"#;
        let resp: GainResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_input_tokens, 50000);
        assert_eq!(resp.total_output_tokens, 10000);
        assert_eq!(resp.total_commands, 100);
        assert_eq!(resp.by_machine.len(), 1);
        assert_eq!(resp.by_machine[0].hostname, "dev-laptop");
        assert_eq!(resp.by_filter.len(), 1);
        assert_eq!(resp.by_filter[0].filter_name.as_deref(), Some("git/push"));
    }

    #[test]
    fn deserialize_gain_response_empty() {
        let json = r#"{
            "total_input_tokens": 0,
            "total_output_tokens": 0,
            "total_commands": 0,
            "by_machine": [],
            "by_filter": []
        }"#;
        let resp: GainResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_commands, 0);
        assert!(resp.by_machine.is_empty());
        assert!(resp.by_filter.is_empty());
    }

    #[test]
    fn deserialize_global_gain_response() {
        let json = r#"{
            "total_input_tokens": 100000,
            "total_output_tokens": 20000,
            "total_commands": 500,
            "by_machine": [{
                "machine_id": "xyz-789",
                "total_input_tokens": 100000,
                "total_output_tokens": 20000,
                "total_commands": 500
            }],
            "by_filter": [{
                "filter_name": null,
                "filter_hash": null,
                "total_input_tokens": 5000,
                "total_output_tokens": 5000,
                "total_commands": 10
            }]
        }"#;
        let resp: GlobalGainResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total_input_tokens, 100_000);
        assert_eq!(resp.by_machine.len(), 1);
        // Global response should not have hostname field
        assert_eq!(resp.by_machine[0].machine_id, "xyz-789");
        assert_eq!(resp.by_filter.len(), 1);
        assert!(resp.by_filter[0].filter_name.is_none());
    }

    #[test]
    fn gain_response_roundtrip_json() {
        let resp = GainResponse {
            total_input_tokens: 1000,
            total_output_tokens: 200,
            total_commands: 5,
            by_machine: vec![MachineGain {
                machine_id: "m1".to_string(),
                hostname: "host".to_string(),
                total_input_tokens: 1000,
                total_output_tokens: 200,
                total_commands: 5,
            }],
            by_filter: vec![FilterGainEntry {
                filter_name: Some("git/status".to_string()),
                filter_hash: Some("h".repeat(64)),
                total_input_tokens: 1000,
                total_output_tokens: 200,
                total_commands: 5,
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        let back: GainResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_input_tokens, 1000);
        assert_eq!(back.by_machine[0].hostname, "host");
    }

    #[test]
    fn filter_gain_entry_null_fields() {
        let json = r#"{
            "filter_name": null,
            "filter_hash": null,
            "total_input_tokens": 100,
            "total_output_tokens": 50,
            "total_commands": 2
        }"#;
        let entry: FilterGainEntry = serde_json::from_str(json).unwrap();
        assert!(entry.filter_name.is_none());
        assert!(entry.filter_hash.is_none());
        assert_eq!(entry.total_input_tokens, 100);
    }
}
