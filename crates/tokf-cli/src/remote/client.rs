use serde::Deserialize;

use super::require_success;

#[derive(Debug, Deserialize)]
pub struct RegisteredMachine {
    pub machine_id: String,
    pub hostname: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct MachineInfo {
    pub machine_id: String,
    pub hostname: String,
    pub created_at: String,
    pub last_sync_at: Option<String>,
}

/// Register this machine with the tokf server via `POST /api/machines`.
///
/// The `client` must have a timeout configured (recommended: 10s request,
/// 5s connect) to avoid hanging indefinitely on an unreachable server.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn register_machine(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
    machine_id: &str,
    hostname: &str,
) -> anyhow::Result<RegisteredMachine> {
    let url = format!("{base_url}/api/machines");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "machine_id": machine_id, "hostname": hostname }))
        .send()
        .map_err(|e| anyhow::anyhow!("could not reach {url}: {e}"))?;

    let resp = require_success(resp)?;
    resp.json::<RegisteredMachine>()
        .map_err(|e| anyhow::anyhow!("invalid response from server: {e}"))
}

/// List machines registered for the authenticated user via `GET /api/machines`.
///
/// The `client` must have a timeout configured (recommended: 10s request,
/// 5s connect) to avoid hanging indefinitely on an unreachable server.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn list_machines(
    client: &reqwest::blocking::Client,
    base_url: &str,
    token: &str,
) -> anyhow::Result<Vec<MachineInfo>> {
    let url = format!("{base_url}/api/machines");
    super::http::authed_get(client, &url, token)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_registered_machine() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z"
        }"#;
        let machine: RegisteredMachine = serde_json::from_str(json).unwrap();
        assert_eq!(machine.machine_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(machine.hostname, "my-laptop");
        assert_eq!(machine.created_at, "2025-01-01T00:00:00Z");
    }

    #[test]
    fn deserialize_machine_info_with_null_last_sync() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z",
            "last_sync_at": null
        }"#;
        let info: MachineInfo = serde_json::from_str(json).unwrap();
        assert!(info.last_sync_at.is_none());
    }

    #[test]
    fn deserialize_machine_info_with_last_sync() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z",
            "last_sync_at": "2025-02-01T12:00:00Z"
        }"#;
        let info: MachineInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.last_sync_at.as_deref(), Some("2025-02-01T12:00:00Z"));
    }
}
